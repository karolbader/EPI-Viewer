#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod entitlements;

use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use pulldown_cmark::{html as markdown_html, Options as MarkdownOptions, Parser as MarkdownParser};
use rfd::FileDialog;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Mutex,
};
use tauri::{Manager, State};
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const REQUIRED_FILES: [&str; 5] = [
    "epi.seal.v1.json",
    "epi.claims.v1.json",
    "epi.drift_report.v1.json",
    "epi.decision_pack.v1.json",
    "epi.runlog.v1.json",
];

const MAX_PREVIEW_BYTES: usize = 1_000_000;

#[derive(Default)]
struct AppState {
    session: Mutex<Option<LoadedSession>>,
}

struct LoadedSession {
    _temp_dir: TempDir,
    extract_root: PathBuf,
    extract_root_canonical: PathBuf,
    pack_path: PathBuf,
    known_paths: BTreeSet<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PackLoadResponse {
    pack_path: String,
    pack_size_bytes: u64,
    pack_sha256: String,
    missing_files: Vec<String>,
    parse_warnings: Vec<String>,
    verification: VerificationSummary,
    quick_counts: QuickCounts,
    claims: Vec<ClaimView>,
    drift: DriftView,
    decision_pack: DecisionPackView,
    files: Vec<PackFileEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuickCounts {
    claims_count: usize,
    drift_changes_count: usize,
    affected_claims_count: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PackFileEntry {
    rel_path: String,
    size_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerificationSummary {
    status: String,
    verifier_path: Option<String>,
    message: Option<String>,
    ok: Option<bool>,
    missing: Option<u64>,
    schema_errors: Option<u64>,
    hash_mismatches: Option<u64>,
    extras: Option<u64>,
    checked_entries_count: Option<u64>,
    timestamp_utc: String,
    raw: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimView {
    claim_id: String,
    title: String,
    impact: String,
    status: String,
    primary_evidence_rel_path: Option<String>,
    evidence: Vec<ClaimEvidenceView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assumptions: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimEvidenceView {
    rel_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct DriftView {
    summary: Value,
    changes: Vec<DriftChangeView>,
    markdown_rel_path: Option<String>,
    markdown_html: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DriftChangeView {
    kind: String,
    entry_path: String,
    summary: Option<Value>,
    affected_claims: Vec<AffectedClaimView>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct AffectedClaimView {
    claim_id: String,
    impact: String,
    changed_paths: Vec<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct DecisionPackView {
    rel_path: Option<String>,
    html: Option<String>,
    pdf_rel_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FilePreviewResponse {
    rel_path: String,
    kind: String,
    text: String,
    html: Option<String>,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PdfPrintResponse {
    printed: bool,
    fallback_opened: bool,
    message: String,
}

#[derive(Serialize, Default, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StartupOptions {
    autostart_pack: Option<String>,
    autostart_tab: Option<String>,
    demo_mode: bool,
}

fn parse_startup_options_from_args<I, S>(args: I) -> StartupOptions
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut parsed = StartupOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_ref() {
            "--pack" => {
                if let Some(value) = iter.next() {
                    parsed.autostart_pack = Some(value.as_ref().to_string());
                }
            }
            "--tab" => {
                if let Some(value) = iter.next() {
                    parsed.autostart_tab = Some(value.as_ref().to_ascii_lowercase());
                }
            }
            "--demo" => {
                parsed.demo_mode = true;
            }
            _ => {}
        }
    }
    parsed
}

fn env_flag_is_true(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

#[tauri::command]
fn get_startup_options() -> StartupOptions {
    let parsed = parse_startup_options_from_args(std::env::args().skip(1));
    StartupOptions {
        autostart_pack: parsed
            .autostart_pack
            .or_else(|| std::env::var("EPI_VIEWER_AUTOSTART_PACK").ok()),
        autostart_tab: parsed.autostart_tab.or_else(|| {
            std::env::var("EPI_VIEWER_AUTOSTART_TAB")
                .ok()
                .map(|value| value.to_ascii_lowercase())
        }),
        demo_mode: parsed.demo_mode || env_flag_is_true("VITE_DEMO"),
    }
}

#[tauri::command]
fn get_entitlements_status() -> entitlements::EntitlementsStatusResponse {
    entitlements::get_entitlements_status(Utc::now())
}

#[tauri::command]
fn pick_pack_zip() -> Result<Option<String>, String> {
    let selected = FileDialog::new()
        .set_title("Open EPI pack.zip")
        .add_filter("EPI Pack", &["zip"])
        .pick_file();
    Ok(selected.map(|path| path_to_string(&path)))
}

#[tauri::command]
fn load_pack(pack_path: String, state: State<'_, AppState>) -> Result<PackLoadResponse, String> {
    let path = PathBuf::from(pack_path);
    let loaded = load_pack_impl(&path).map_err(format_error)?;
    let mut guard = state.session.lock().map_err(|_| "state lock poisoned")?;
    *guard = Some(loaded.session);
    Ok(loaded.response)
}

#[tauri::command]
fn read_file_preview(
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<FilePreviewResponse, String> {
    let guard = state.session.lock().map_err(|_| "state lock poisoned")?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "No pack loaded yet".to_string())?;
    let (resolved_rel, canonical_path) =
        resolve_session_file_path(session, &rel_path).map_err(format_error)?;
    build_preview(&resolved_rel, &canonical_path).map_err(format_error)
}

#[tauri::command]
fn open_pack_file(rel_path: String, state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.session.lock().map_err(|_| "state lock poisoned")?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "No pack loaded yet".to_string())?;
    let (_, canonical_path) =
        resolve_session_file_path(session, &rel_path).map_err(format_error)?;
    open_path_with_system(&canonical_path).map_err(format_error)
}

#[tauri::command]
fn export_pack_file(
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let guard = state.session.lock().map_err(|_| "state lock poisoned")?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "No pack loaded yet".to_string())?;
    let (resolved_rel, canonical_path) =
        resolve_session_file_path(session, &rel_path).map_err(format_error)?;
    let default_name = Path::new(&resolved_rel)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export.bin");
    let save_path = FileDialog::new()
        .set_title("Export file")
        .set_file_name(default_name)
        .save_file();
    let Some(save_path) = save_path else {
        return Ok(None);
    };
    fs::copy(&canonical_path, &save_path)
        .with_context(|| {
            format!(
                "failed to export {} to {}",
                canonical_path.display(),
                save_path.display()
            )
        })
        .map_err(format_error)?;
    Ok(Some(path_to_string(&save_path)))
}

#[tauri::command]
fn print_pack_pdf(
    rel_path: String,
    state: State<'_, AppState>,
) -> Result<PdfPrintResponse, String> {
    let guard = state.session.lock().map_err(|_| "state lock poisoned")?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "No pack loaded yet".to_string())?;
    let (resolved_rel, canonical_path) =
        resolve_session_file_path(session, &rel_path).map_err(format_error)?;
    let is_pdf = Path::new(&resolved_rel)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"));
    if !is_pdf {
        return Err(format!(
            "Print is only supported for PDF files: {resolved_rel}"
        ));
    }

    match print_path_with_system(&canonical_path) {
        Ok(()) => Ok(PdfPrintResponse {
            printed: true,
            fallback_opened: false,
            message: "Print command sent to system PDF handler.".to_string(),
        }),
        Err(print_err) => {
            open_path_with_system(&canonical_path).map_err(format_error)?;
            Ok(PdfPrintResponse {
                printed: false,
                fallback_opened: true,
                message: format!(
                    "Print command failed ({print_err}). Opened PDF instead. Use Ctrl+P to print."
                ),
            })
        }
    }
}

#[tauri::command]
fn export_support_pack(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let snapshot = {
        let guard = state.session.lock().map_err(|_| "state lock poisoned")?;
        let session = guard
            .as_ref()
            .ok_or_else(|| "No pack loaded yet".to_string())?;
        SessionSnapshot {
            extract_root: session.extract_root.clone(),
            extract_root_canonical: session.extract_root_canonical.clone(),
            pack_path: session.pack_path.clone(),
            known_paths: session.known_paths.clone(),
        }
    };

    let default_name = format!(
        "Civitas-EPI-Viewer-SupportPack-{}.zip",
        Utc::now().format("%Y%m%d-%H%M%S")
    );
    let save_path = FileDialog::new()
        .set_title("Export Support Pack")
        .add_filter("Support Pack", &["zip"])
        .set_file_name(&default_name)
        .save_file();
    let Some(save_path) = save_path else {
        return Ok(None);
    };

    write_support_pack_zip(&snapshot, &save_path).map_err(format_error)?;
    Ok(Some(path_to_string(&save_path)))
}

struct LoadedPack {
    response: PackLoadResponse,
    session: LoadedSession,
}

#[derive(Clone)]
struct SessionSnapshot {
    extract_root: PathBuf,
    extract_root_canonical: PathBuf,
    pack_path: PathBuf,
    known_paths: BTreeSet<String>,
}

fn normalize_and_validate_rel_path(rel_path: &str) -> Result<String> {
    let sanitized = rel_path.trim().replace('\\', "/");
    if sanitized.is_empty() {
        bail!("Relative path is empty");
    }

    let path = Path::new(&sanitized);
    if path.is_absolute() {
        bail!("Absolute paths are not allowed: {rel_path}");
    }

    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => bail!("Parent traversal is not allowed: {rel_path}"),
            Component::Prefix(_) | Component::RootDir => {
                bail!("Absolute paths are not allowed: {rel_path}")
            }
        }
    }

    let normalized = normalize_rel_path(path);
    if normalized.is_empty() {
        bail!("Relative path is empty");
    }
    Ok(normalized)
}

fn resolve_known_rel_path(known_paths: &BTreeSet<String>, rel_path: &str) -> Result<String> {
    let normalized = normalize_and_validate_rel_path(rel_path)?;
    if known_paths.contains(&normalized) {
        return Ok(normalized);
    }

    if cfg!(target_os = "windows") {
        if let Some(existing) = known_paths
            .iter()
            .find(|existing| existing.eq_ignore_ascii_case(&normalized))
        {
            return Ok(existing.clone());
        }
    }

    bail!("File is not part of current pack: {normalized}");
}

fn resolve_file_under_extract_root(
    extract_root: &Path,
    extract_root_canonical: &Path,
    rel_path: &str,
) -> Result<PathBuf> {
    let full_path = extract_root.join(Path::new(rel_path));
    let canonical = full_path
        .canonicalize()
        .with_context(|| format!("failed to resolve file: {}", full_path.display()))?;
    if !canonical.starts_with(extract_root_canonical) {
        bail!("Refusing to read path outside extracted pack");
    }
    Ok(canonical)
}

fn resolve_session_file_path(session: &LoadedSession, rel_path: &str) -> Result<(String, PathBuf)> {
    let resolved_rel = resolve_known_rel_path(&session.known_paths, rel_path)?;
    let canonical = resolve_file_under_extract_root(
        &session.extract_root,
        &session.extract_root_canonical,
        &resolved_rel,
    )?;
    Ok((resolved_rel, canonical))
}

fn load_pack_impl(pack_path: &Path) -> Result<LoadedPack> {
    if !pack_path.is_file() {
        bail!("pack path is not a file: {}", pack_path.display());
    }
    if !pack_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        bail!("expected a .zip file: {}", pack_path.display());
    }

    let canonical_pack = pack_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize pack path: {}", pack_path.display()))?;
    let pack_size = fs::metadata(&canonical_pack)
        .with_context(|| format!("failed to read metadata: {}", canonical_pack.display()))?
        .len();
    let pack_sha256 = sha256_file(&canonical_pack)?;

    let temp_dir = tempfile::Builder::new()
        .prefix("epi-viewer-")
        .tempdir()
        .context("failed to create session temp directory")?;
    let extract_root = temp_dir.path().join("pack");
    fs::create_dir_all(&extract_root).with_context(|| {
        format!(
            "failed to create extraction dir: {}",
            extract_root.display()
        )
    })?;

    let mut parse_warnings = extract_zip_to_dir(&canonical_pack, &extract_root)?;
    let files = collect_extracted_files(&extract_root)?;
    let extract_root_canonical = extract_root.canonicalize().with_context(|| {
        format!(
            "failed to resolve extraction dir: {}",
            extract_root.display()
        )
    })?;
    let known_paths: BTreeSet<String> = files.iter().map(|entry| entry.rel_path.clone()).collect();

    let missing_files = REQUIRED_FILES
        .iter()
        .filter(|required| !known_paths.contains(**required))
        .map(|required| (*required).to_string())
        .collect::<Vec<String>>();

    let _seal_json =
        read_optional_json_value(&extract_root, "epi.seal.v1.json", &mut parse_warnings);
    let claims_json =
        read_optional_json_value(&extract_root, "epi.claims.v1.json", &mut parse_warnings);
    let drift_json = read_optional_json_value(
        &extract_root,
        "epi.drift_report.v1.json",
        &mut parse_warnings,
    );
    let _decision_pack_json = read_optional_json_value(
        &extract_root,
        "epi.decision_pack.v1.json",
        &mut parse_warnings,
    );
    let _runlog_json =
        read_optional_json_value(&extract_root, "epi.runlog.v1.json", &mut parse_warnings);

    let claims = claims_json
        .as_ref()
        .map(parse_claims_from_value)
        .unwrap_or_default();

    let mut drift = drift_json
        .as_ref()
        .map(parse_drift_from_value)
        .unwrap_or_default();
    if let Some(markdown_rel_path) = files
        .iter()
        .find(|entry| file_name_matches(&entry.rel_path, "DRIFT.md"))
        .map(|entry| entry.rel_path.clone())
    {
        let markdown_path = extract_root.join(Path::new(&markdown_rel_path));
        if let Ok(markdown_text) = fs::read_to_string(&markdown_path) {
            drift.markdown_rel_path = Some(markdown_rel_path);
            drift.markdown_html = Some(markdown_to_html(&markdown_text));
        }
    }

    let decision_pack = build_decision_pack_view(&extract_root, &files);
    let verification = run_verifier(&canonical_pack);
    let quick_counts = build_quick_counts(&claims, &drift);

    let response = PackLoadResponse {
        pack_path: path_to_string(&canonical_pack),
        pack_size_bytes: pack_size,
        pack_sha256,
        missing_files,
        parse_warnings,
        verification,
        quick_counts,
        claims,
        drift,
        decision_pack,
        files: files.clone(),
    };

    Ok(LoadedPack {
        response,
        session: LoadedSession {
            _temp_dir: temp_dir,
            extract_root,
            extract_root_canonical,
            pack_path: canonical_pack,
            known_paths,
        },
    })
}

fn build_quick_counts(claims: &[ClaimView], drift: &DriftView) -> QuickCounts {
    let affected_claims_count = drift
        .summary
        .get("claims_affected_count")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or_else(|| {
            let mut unique_ids = BTreeSet::new();
            for change in &drift.changes {
                for claim in &change.affected_claims {
                    if !claim.claim_id.is_empty() {
                        unique_ids.insert(claim.claim_id.clone());
                    }
                }
            }
            unique_ids.len()
        });

    QuickCounts {
        claims_count: claims.len(),
        drift_changes_count: drift.changes.len(),
        affected_claims_count,
    }
}

fn extract_zip_to_dir(zip_path: &Path, extract_root: &Path) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let file = fs::File::open(zip_path)
        .with_context(|| format!("failed to open zip: {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive: {}", zip_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("failed to read zip entry #{index}"))?;

        let Some(safe_rel) = entry.enclosed_name().map(|value| value.to_path_buf()) else {
            warnings.push(format!("skipped unsafe zip entry: {}", entry.name()));
            continue;
        };

        let out_path = extract_root.join(&safe_rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("failed to create dir: {}", out_path.display()))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dir: {}", parent.display()))?;
        }

        let mut out_file = fs::File::create(&out_path)
            .with_context(|| format!("failed to create output file: {}", out_path.display()))?;
        std::io::copy(&mut entry, &mut out_file)
            .with_context(|| format!("failed to extract file: {}", out_path.display()))?;
    }

    Ok(warnings)
}

fn collect_extracted_files(root: &Path) -> Result<Vec<PackFileEntry>> {
    let mut entries = Vec::new();
    for dir_entry in WalkDir::new(root).follow_links(false) {
        let dir_entry =
            dir_entry.with_context(|| format!("walkdir failed for {}", root.display()))?;
        if !dir_entry.file_type().is_file() {
            continue;
        }
        let path = dir_entry.path();
        let rel = path
            .strip_prefix(root)
            .with_context(|| format!("failed to build relative path for {}", path.display()))?;
        let rel_path = normalize_rel_path(rel);
        let size_bytes = dir_entry
            .metadata()
            .with_context(|| format!("failed to read metadata for {}", path.display()))?
            .len();
        entries.push(PackFileEntry {
            rel_path,
            size_bytes,
        });
    }
    entries.sort_by(|left, right| {
        left.rel_path
            .to_ascii_lowercase()
            .cmp(&right.rel_path.to_ascii_lowercase())
            .then_with(|| left.rel_path.cmp(&right.rel_path))
    });
    Ok(entries)
}

fn normalize_rel_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        if let Component::Normal(value) = component {
            parts.push(value.to_string_lossy().to_string());
        }
    }
    parts.join("/")
}

fn read_optional_json_value(
    root: &Path,
    rel_path: &str,
    parse_warnings: &mut Vec<String>,
) -> Option<Value> {
    let full_path = root.join(Path::new(rel_path));
    if !full_path.is_file() {
        return None;
    }

    let text = match fs::read_to_string(&full_path) {
        Ok(value) => value,
        Err(err) => {
            parse_warnings.push(format!("{rel_path}: failed to read ({err})"));
            return None;
        }
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => Some(value),
        Err(err) => {
            parse_warnings.push(format!("{rel_path}: invalid JSON ({err})"));
            None
        }
    }
}

fn parse_claims_from_value(root: &Value) -> Vec<ClaimView> {
    let mut rows = Vec::new();
    let Some(claims) = root.get("claims").and_then(Value::as_array) else {
        return rows;
    };

    for claim in claims {
        let claim_id = value_string(claim.get("claim_id")).unwrap_or_default();
        let title = value_string(claim.get("title")).unwrap_or_default();
        let impact = value_string(claim.get("impact")).unwrap_or_default();
        let status = value_string(claim.get("status")).unwrap_or_default();

        let mut evidence = Vec::new();
        if let Some(items) = claim.get("evidence").and_then(Value::as_array) {
            for item in items {
                let rel_path = value_string(item.get("rel_path")).unwrap_or_default();
                if rel_path.is_empty() {
                    continue;
                }
                evidence.push(ClaimEvidenceView {
                    rel_path,
                    sha256: value_string(item.get("sha256")),
                });
            }
        }
        evidence.sort_by(|left, right| {
            left.rel_path
                .to_ascii_lowercase()
                .cmp(&right.rel_path.to_ascii_lowercase())
                .then_with(|| left.rel_path.cmp(&right.rel_path))
        });

        let primary_evidence_rel_path = evidence
            .first()
            .map(|entry| entry.rel_path.clone())
            .or_else(|| {
                claim
                    .get("evidence_refs")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            });

        rows.push(ClaimView {
            claim_id,
            title,
            impact,
            status,
            primary_evidence_rel_path,
            evidence,
            notes: optional_value_to_string(claim.get("notes")),
            assumptions: optional_value_to_string(claim.get("assumptions")),
        });
    }

    rows.sort_by(|left, right| {
        left.claim_id
            .to_ascii_lowercase()
            .cmp(&right.claim_id.to_ascii_lowercase())
            .then_with(|| left.claim_id.cmp(&right.claim_id))
    });
    rows
}

fn parse_drift_from_value(root: &Value) -> DriftView {
    let summary = root
        .get("drift_summary")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut changes = Vec::new();

    if let Some(items) = root.get("changes").and_then(Value::as_array) {
        for item in items {
            let kind = value_string(item.get("kind")).unwrap_or_default();
            let entry_path = value_string(item.get("entry_path")).unwrap_or_default();
            let summary_value = item.get("summary").cloned();
            let mut affected_claims = extract_affected_claims(item);
            affected_claims.sort_by(|left, right| {
                left.claim_id
                    .to_ascii_lowercase()
                    .cmp(&right.claim_id.to_ascii_lowercase())
                    .then_with(|| left.claim_id.cmp(&right.claim_id))
            });

            changes.push(DriftChangeView {
                kind,
                entry_path,
                summary: summary_value,
                affected_claims,
            });
        }
    }

    changes.sort_by(|left, right| {
        left.entry_path
            .to_ascii_lowercase()
            .cmp(&right.entry_path.to_ascii_lowercase())
            .then_with(|| left.entry_path.cmp(&right.entry_path))
            .then_with(|| {
                left.kind
                    .to_ascii_lowercase()
                    .cmp(&right.kind.to_ascii_lowercase())
            })
            .then_with(|| left.kind.cmp(&right.kind))
    });

    DriftView {
        summary,
        changes,
        markdown_rel_path: None,
        markdown_html: None,
    }
}

fn extract_affected_claims(change: &Value) -> Vec<AffectedClaimView> {
    let sources = [
        change.get("affected_claims"),
        change.pointer("/summary/affected_claims"),
    ];

    for source in sources.into_iter().flatten() {
        if let Some(items) = source.as_array() {
            return items
                .iter()
                .map(|item| AffectedClaimView {
                    claim_id: value_string(item.get("claim_id")).unwrap_or_default(),
                    impact: value_string(item.get("impact")).unwrap_or_default(),
                    changed_paths: value_string_array(item.get("changed_paths")),
                })
                .collect::<Vec<AffectedClaimView>>();
        }
    }

    Vec::new()
}

fn build_decision_pack_view(root: &Path, files: &[PackFileEntry]) -> DecisionPackView {
    let html_rel_path = files
        .iter()
        .find(|entry| file_name_matches(&entry.rel_path, "DecisionPack.html"))
        .map(|entry| entry.rel_path.clone());
    let pdf_rel_path = files
        .iter()
        .find(|entry| file_name_matches(&entry.rel_path, "DecisionPack.pdf"))
        .map(|entry| entry.rel_path.clone());

    let html = html_rel_path
        .as_ref()
        .and_then(|rel_path| fs::read_to_string(root.join(Path::new(rel_path))).ok());

    DecisionPackView {
        rel_path: html_rel_path,
        html,
        pdf_rel_path,
    }
}

fn run_verifier(pack_path: &Path) -> VerificationSummary {
    let timestamp_utc = now_rfc3339_utc();
    let Some(verifier_path) = find_epi_cli() else {
        return VerificationSummary {
            status: "disabled".to_string(),
            verifier_path: None,
            message: Some("Verifier not found".to_string()),
            ok: None,
            missing: None,
            schema_errors: None,
            hash_mismatches: None,
            extras: None,
            checked_entries_count: None,
            timestamp_utc,
            raw: None,
        };
    };

    let output = Command::new(&verifier_path)
        .arg("verify")
        .arg(pack_path)
        .arg("--json")
        .output();
    let output = match output {
        Ok(value) => value,
        Err(err) => {
            return VerificationSummary {
                status: "error".to_string(),
                verifier_path: Some(path_to_string(&verifier_path)),
                message: Some(format!("failed to run verifier: {err}")),
                ok: None,
                missing: None,
                schema_errors: None,
                hash_mismatches: None,
                extras: None,
                checked_entries_count: None,
                timestamp_utc,
                raw: None,
            };
        }
    };

    let stdout_text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr_text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        let details = if !stderr_text.is_empty() {
            stderr_text
        } else if !stdout_text.is_empty() {
            stdout_text
        } else {
            format!("verifier exit code {:?}", output.status.code())
        };
        return VerificationSummary {
            status: "error".to_string(),
            verifier_path: Some(path_to_string(&verifier_path)),
            message: Some(details),
            ok: None,
            missing: None,
            schema_errors: None,
            hash_mismatches: None,
            extras: None,
            checked_entries_count: None,
            timestamp_utc,
            raw: None,
        };
    }

    let parsed = match serde_json::from_str::<Value>(&stdout_text) {
        Ok(value) => value,
        Err(err) => {
            return VerificationSummary {
                status: "error".to_string(),
                verifier_path: Some(path_to_string(&verifier_path)),
                message: Some(format!("verifier output is not valid JSON: {err}")),
                ok: None,
                missing: None,
                schema_errors: None,
                hash_mismatches: None,
                extras: None,
                checked_entries_count: None,
                timestamp_utc,
                raw: None,
            };
        }
    };

    let ok = parsed
        .pointer("/status/success")
        .and_then(Value::as_bool)
        .or_else(|| parsed.get("ok").and_then(Value::as_bool))
        .unwrap_or(false);
    let missing = count_field(&parsed, "missing_files").or_else(|| count_field(&parsed, "missing"));
    let schema_errors = [
        count_field(&parsed, "schema_version_mismatches"),
        count_field(&parsed, "invalid_json"),
        count_field(&parsed, "schema_errors"),
    ]
    .into_iter()
    .flatten()
    .sum::<u64>();
    let hash_mismatches =
        count_field(&parsed, "hash_mismatches").or_else(|| count_field(&parsed, "hash_errors"));
    let extras = count_field(&parsed, "extras");
    let checked_entries_count = parsed
        .get("checked_entries_count")
        .and_then(Value::as_u64)
        .or_else(|| {
            parsed
                .get("file_hashes")
                .and_then(Value::as_object)
                .map(|map| map.len() as u64)
        });
    let timestamp_from_verifier = parsed
        .get("timestamp_utc")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    VerificationSummary {
        status: if ok {
            "ok".to_string()
        } else {
            "failed".to_string()
        },
        verifier_path: Some(path_to_string(&verifier_path)),
        message: None,
        ok: Some(ok),
        missing,
        schema_errors: Some(schema_errors),
        hash_mismatches,
        extras,
        checked_entries_count,
        timestamp_utc: timestamp_from_verifier.unwrap_or(timestamp_utc),
        raw: Some(parsed),
    }
}

fn count_field(root: &Value, key: &str) -> Option<u64> {
    root.get(key).and_then(|value| match value {
        Value::Array(items) => Some(items.len() as u64),
        Value::Object(items) => Some(items.len() as u64),
        Value::Number(number) => number.as_u64(),
        _ => None,
    })
}

fn find_epi_cli() -> Option<PathBuf> {
    let file_name = epi_cli_file_name();

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled = exe_dir.join("tools").join("epi").join(file_name);
            if bundled.is_file() {
                return Some(bundled);
            }
        }
    }

    if let Some(env_path) = std::env::var_os("EPI_CLI_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(env_path);
    }

    None
}

fn epi_cli_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "epi-cli.exe"
    } else {
        "epi-cli"
    }
}

#[cfg(target_os = "windows")]
fn open_path_with_system(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("file not found: {}", path.display());
    }
    windows_start_process(path, None)
}

#[cfg(target_os = "macos")]
fn open_path_with_system(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("file not found: {}", path.display());
    }
    let status = Command::new("open")
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch file with open: {}", path.display()))?;
    if !status.success() {
        bail!("open exited with {:?}", status.code());
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_path_with_system(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("file not found: {}", path.display());
    }
    let status = Command::new("xdg-open")
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch file with xdg-open: {}", path.display()))?;
    if !status.success() {
        bail!("xdg-open exited with {:?}", status.code());
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
fn open_path_with_system(path: &Path) -> Result<()> {
    let _ = path;
    bail!("Open action is not supported on this platform")
}

#[cfg(target_os = "windows")]
fn print_path_with_system(path: &Path) -> Result<()> {
    windows_start_process(path, Some("Print"))
}

#[cfg(not(target_os = "windows"))]
fn print_path_with_system(path: &Path) -> Result<()> {
    let _ = path;
    bail!("Print action is not supported on this platform")
}

#[cfg(target_os = "windows")]
fn windows_start_process(path: &Path, verb: Option<&str>) -> Result<()> {
    let mut command = Command::new("powershell");
    command.arg("-NoProfile");
    if let Some(verb) = verb {
        command
            .arg("-Command")
            .arg("Start-Process -FilePath $args[0] -Verb $args[1]")
            .arg("--")
            .arg(path)
            .arg(verb);
    } else {
        command
            .arg("-Command")
            .arg("Start-Process -FilePath $args[0]")
            .arg("--")
            .arg(path);
    }

    let status = command
        .status()
        .with_context(|| format!("failed to start process for {}", path.display()))?;
    if !status.success() {
        bail!("system process exited with {:?}", status.code());
    }
    Ok(())
}

fn write_support_pack_zip(snapshot: &SessionSnapshot, out_zip_path: &Path) -> Result<()> {
    if let Some(parent) = out_zip_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create support pack output dir: {}",
                parent.display()
            )
        })?;
    }

    let out_file = fs::File::create(out_zip_path)
        .with_context(|| format!("failed to create zip file: {}", out_zip_path.display()))?;
    let mut zip = ZipWriter::new(out_file);

    if snapshot.pack_path.is_file() {
        add_file_to_zip(&mut zip, &snapshot.pack_path, "input/pack.zip")?;
    } else {
        for rel_path in &snapshot.known_paths {
            add_session_path_to_zip(
                &mut zip,
                snapshot,
                rel_path,
                &format!("input/extracted/{rel_path}"),
            )?;
        }
    }

    if let Some(rel_path) = find_first_rel_path_by_file_name(&snapshot.known_paths, "verify.json") {
        add_session_path_to_zip(&mut zip, snapshot, &rel_path, "artifacts/verify.json")?;
    }
    if let Some(rel_path) =
        find_first_rel_path_by_file_name(&snapshot.known_paths, "DecisionPack.html")
    {
        add_session_path_to_zip(&mut zip, snapshot, &rel_path, "artifacts/DecisionPack.html")?;
    }
    if let Some(rel_path) =
        find_first_rel_path_by_file_name(&snapshot.known_paths, "DecisionPack.pdf")
    {
        add_session_path_to_zip(&mut zip, snapshot, &rel_path, "artifacts/DecisionPack.pdf")?;
    }

    if let Some(log_path) = find_optional_viewer_log() {
        add_file_to_zip(&mut zip, &log_path, "diagnostics/viewer.log")?;
    }

    let env_text = format!(
        "app_name=epi-viewer\napp_version={}\nos={}\narch={}\ntimestamp_utc={}\npack_path={}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        now_rfc3339_utc(),
        path_to_string(&snapshot.pack_path)
    );
    add_text_to_zip(&mut zip, "env.txt", &env_text)?;

    zip.finish()
        .context("failed to finalize support pack zip")?;
    Ok(())
}

fn add_session_path_to_zip(
    zip: &mut ZipWriter<fs::File>,
    snapshot: &SessionSnapshot,
    rel_path: &str,
    zip_rel_path: &str,
) -> Result<()> {
    let canonical = resolve_file_under_extract_root(
        &snapshot.extract_root,
        &snapshot.extract_root_canonical,
        rel_path,
    )?;
    add_file_to_zip(zip, &canonical, zip_rel_path)
}

fn add_file_to_zip(
    zip: &mut ZipWriter<fs::File>,
    source_path: &Path,
    zip_rel_path: &str,
) -> Result<()> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    zip.start_file(zip_rel_path.replace('\\', "/"), options)
        .with_context(|| format!("failed to create zip entry {zip_rel_path}"))?;
    let mut input = fs::File::open(source_path)
        .with_context(|| format!("failed to open source file {}", source_path.display()))?;
    std::io::copy(&mut input, zip)
        .with_context(|| format!("failed to copy source file {}", source_path.display()))?;
    Ok(())
}

fn add_text_to_zip(zip: &mut ZipWriter<fs::File>, zip_rel_path: &str, text: &str) -> Result<()> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    zip.start_file(zip_rel_path.replace('\\', "/"), options)
        .with_context(|| format!("failed to create zip entry {zip_rel_path}"))?;
    zip.write_all(text.as_bytes())
        .with_context(|| format!("failed to write zip entry {zip_rel_path}"))?;
    Ok(())
}

fn find_first_rel_path_by_file_name(
    known_paths: &BTreeSet<String>,
    file_name: &str,
) -> Option<String> {
    known_paths
        .iter()
        .find(|rel_path| file_name_matches(rel_path, file_name))
        .cloned()
}

fn find_optional_viewer_log() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("viewer.log"));
            candidates.push(exe_dir.join("logs").join("viewer.log"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("viewer.log"));
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn build_preview(rel_path: &str, full_path: &Path) -> Result<FilePreviewResponse> {
    let bytes = fs::read(full_path)
        .with_context(|| format!("failed to read file: {}", full_path.display()))?;
    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    let slice = if truncated {
        &bytes[..MAX_PREVIEW_BYTES]
    } else {
        &bytes[..]
    };

    let extension = full_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if extension == "json" {
        let text = String::from_utf8_lossy(slice).to_string();
        let pretty = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .unwrap_or(text);
        return Ok(FilePreviewResponse {
            rel_path: rel_path.to_string(),
            kind: "json".to_string(),
            text: pretty,
            html: None,
            truncated,
        });
    }

    if extension == "md" || extension == "markdown" {
        let text = String::from_utf8_lossy(slice).to_string();
        return Ok(FilePreviewResponse {
            rel_path: rel_path.to_string(),
            kind: "markdown".to_string(),
            html: Some(markdown_to_html(&text)),
            text,
            truncated,
        });
    }

    if extension == "html" || extension == "htm" {
        let text = String::from_utf8_lossy(slice).to_string();
        return Ok(FilePreviewResponse {
            rel_path: rel_path.to_string(),
            kind: "html".to_string(),
            html: Some(text.clone()),
            text,
            truncated,
        });
    }

    if extension == "txt" || extension == "log" {
        return Ok(FilePreviewResponse {
            rel_path: rel_path.to_string(),
            kind: "text".to_string(),
            text: String::from_utf8_lossy(slice).to_string(),
            html: None,
            truncated,
        });
    }

    if extension == "pdf" {
        return Ok(FilePreviewResponse {
            rel_path: rel_path.to_string(),
            kind: "pdf".to_string(),
            text: String::new(),
            html: None,
            truncated,
        });
    }

    Ok(FilePreviewResponse {
        rel_path: rel_path.to_string(),
        kind: "unsupported".to_string(),
        text: String::new(),
        html: None,
        truncated,
    })
}

fn markdown_to_html(markdown: &str) -> String {
    let mut options = MarkdownOptions::empty();
    options.insert(MarkdownOptions::ENABLE_TABLES);
    options.insert(MarkdownOptions::ENABLE_STRIKETHROUGH);
    options.insert(MarkdownOptions::ENABLE_TASKLISTS);

    let parser = MarkdownParser::new_ext(markdown, options);
    let mut output = String::new();
    markdown_html::push_html(&mut output, parser);
    output
}

fn file_name_matches(rel_path: &str, wanted_name: &str) -> bool {
    Path::new(rel_path)
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(wanted_name))
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

fn value_string_array(value: Option<&Value>) -> Vec<String> {
    let mut values = value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    values.sort_by(|left, right| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    values
}

fn optional_value_to_string(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::Null => None,
        Value::String(text) => {
            if text.trim().is_empty() {
                None
            } else {
                Some(text.clone())
            }
        }
        _ => serde_json::to_string_pretty(value).ok(),
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open file for SHA256: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65_536];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read file for SHA256: {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn now_rfc3339_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn format_error(err: impl std::fmt::Display) -> String {
    err.to_string()
}

fn clear_session_on_exit(app_handle: &tauri::AppHandle) {
    if let Some(state) = app_handle.try_state::<AppState>() {
        if let Ok(mut guard) = state.session.lock() {
            *guard = None;
        }
    }
}

fn register_drop_bridge(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let callback_window = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::DragDrop(drop_event) = event {
            if let tauri::DragDropEvent::Drop { paths, .. } = drop_event {
                let payload = paths
                    .iter()
                    .map(|path| path_to_string(path))
                    .collect::<Vec<String>>();
                if let Ok(json_payload) = serde_json::to_string(&payload) {
                    let script = format!(
                        "if (window.__EPI_VIEWER_DROP__) window.__EPI_VIEWER_DROP__({json_payload});"
                    );
                    let _ = callback_window.eval(&script);
                }
            }
        }
    });
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            register_drop_bridge(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_startup_options,
            get_entitlements_status,
            pick_pack_zip,
            load_pack,
            read_file_preview,
            open_pack_file,
            export_pack_file,
            print_pack_pdf,
            export_support_pack
        ])
        .build(tauri::generate_context!())
        .expect("failed to build tauri app")
        .run(|app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
            ) {
                clear_session_on_exit(app_handle);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::io::Write;
    use std::sync::Mutex;
    use zip::write::SimpleFileOptions;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn sort_paths_deterministically(paths: &mut [String]) {
        paths.sort_by(|left, right| {
            left.to_ascii_lowercase()
                .cmp(&right.to_ascii_lowercase())
                .then_with(|| left.cmp(right))
        });
    }

    fn list_zip_entries_sorted(zip_path: &Path) -> Result<Vec<String>> {
        let file = fs::File::open(zip_path)?;
        let mut archive = ZipArchive::new(file)?;
        let mut names = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            if let Some(path) = entry.enclosed_name() {
                names.push(normalize_rel_path(&path));
            }
        }
        sort_paths_deterministically(&mut names);
        Ok(names)
    }

    #[test]
    fn zip_listing_is_sorted_deterministically() -> Result<()> {
        let temp_dir = tempfile::Builder::new()
            .prefix("epi-viewer-test-")
            .tempdir()?;
        let zip_path = temp_dir.path().join("pack.zip");
        let file = fs::File::create(&zip_path)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();

        zip.start_file("b/Z.txt", options)?;
        zip.write_all(b"1")?;
        zip.start_file("A/a.txt", options)?;
        zip.write_all(b"2")?;
        zip.start_file("a/B.txt", options)?;
        zip.write_all(b"3")?;
        zip.finish()?;

        let listed = list_zip_entries_sorted(&zip_path)?;
        assert_eq!(listed, vec!["A/a.txt", "a/B.txt", "b/Z.txt"]);
        Ok(())
    }

    #[test]
    fn parsing_claims_and_drift_with_missing_optional_fields_is_safe() -> Result<()> {
        let claims_json = serde_json::from_str::<Value>(
            r#"{
              "schema_version": "epi.claims.v1",
              "claims": [
                { "claim_id": "CLAIM-1", "title": "T", "status": "unknown", "impact": "low" }
              ]
            }"#,
        )?;
        let drift_json = serde_json::from_str::<Value>(
            r#"{
              "schema_version": "epi.drift_report.v1",
              "changes": [
                { "kind": "modified", "entry_path": "x.txt" }
              ]
            }"#,
        )?;

        let claims = parse_claims_from_value(&claims_json);
        let drift = parse_drift_from_value(&drift_json);

        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].claim_id, "CLAIM-1");
        assert_eq!(drift.changes.len(), 1);
        assert_eq!(drift.changes[0].entry_path, "x.txt");
        Ok(())
    }

    #[test]
    fn startup_options_parse_from_cli_args() {
        let parsed = parse_startup_options_from_args(vec![
            "--pack",
            r"E:\_packs\demo\pack.zip",
            "--tab",
            "Claims",
            "--demo",
        ]);
        assert_eq!(
            parsed.autostart_pack.as_deref(),
            Some(r"E:\_packs\demo\pack.zip")
        );
        assert_eq!(parsed.autostart_tab.as_deref(), Some("claims"));
        assert!(parsed.demo_mode);
    }

    #[test]
    fn resolve_session_file_path_keeps_access_inside_extract_root() -> Result<()> {
        let temp_dir = tempfile::Builder::new()
            .prefix("epi-viewer-session-")
            .tempdir()?;
        let extract_root = temp_dir.path().join("pack");
        let file_rel = "docs/Quote.json";
        let file_path = extract_root.join(Path::new(file_rel));
        fs::create_dir_all(file_path.parent().unwrap())?;
        fs::write(&file_path, br#"{"ok":true}"#)?;
        let extract_root_canonical = extract_root.canonicalize()?;

        let mut known_paths = BTreeSet::new();
        known_paths.insert(file_rel.to_string());
        let session = LoadedSession {
            _temp_dir: temp_dir,
            extract_root: extract_root.clone(),
            extract_root_canonical: extract_root_canonical.clone(),
            pack_path: extract_root.join("pack.zip"),
            known_paths,
        };

        let (resolved_rel, canonical_file) =
            resolve_session_file_path(&session, r"docs\Quote.json")?;
        assert_eq!(resolved_rel, file_rel);
        assert!(canonical_file.starts_with(&extract_root_canonical));

        let traversal_err = resolve_session_file_path(&session, "../outside.txt").unwrap_err();
        assert!(
            traversal_err.to_string().contains("Parent traversal"),
            "unexpected error: {traversal_err}"
        );
        Ok(())
    }

    #[test]
    fn support_pack_zip_contains_expected_entries() -> Result<()> {
        let temp_dir = tempfile::Builder::new()
            .prefix("epi-viewer-support-")
            .tempdir()?;
        let extract_root = temp_dir.path().join("pack");
        fs::create_dir_all(&extract_root)?;
        fs::write(extract_root.join("verify.json"), b"{\"ok\":true}")?;
        fs::write(extract_root.join("DecisionPack.html"), b"<html></html>")?;
        fs::write(extract_root.join("DecisionPack.pdf"), b"%PDF-1.7")?;

        let pack_zip = temp_dir.path().join("pack.zip");
        fs::write(&pack_zip, b"zip-bytes")?;

        let mut known_paths = BTreeSet::new();
        known_paths.insert("verify.json".to_string());
        known_paths.insert("DecisionPack.html".to_string());
        known_paths.insert("DecisionPack.pdf".to_string());

        let snapshot = SessionSnapshot {
            extract_root: extract_root.clone(),
            extract_root_canonical: extract_root.canonicalize()?,
            pack_path: pack_zip,
            known_paths,
        };
        let out_zip = temp_dir.path().join("support.zip");
        write_support_pack_zip(&snapshot, &out_zip)?;

        let entries = list_zip_entries_sorted(&out_zip)?;
        assert!(entries.contains(&"input/pack.zip".to_string()));
        assert!(entries.contains(&"artifacts/verify.json".to_string()));
        assert!(entries.contains(&"artifacts/DecisionPack.html".to_string()));
        assert!(entries.contains(&"artifacts/DecisionPack.pdf".to_string()));
        assert!(entries.contains(&"env.txt".to_string()));
        Ok(())
    }

    #[test]
    fn iso_pack_text_previews_load_when_pack_is_available() -> Result<()> {
        let pack_path = PathBuf::from(r"E:\_packs\DEMO_iso27001-lite-v1_PACK-001\pack.zip");
        if !pack_path.is_file() {
            return Ok(());
        }

        let loaded = load_pack_impl(&pack_path)?;
        let session = loaded.session;

        let quote_rel = session
            .known_paths
            .iter()
            .find(|rel| file_name_matches(rel, "Quote.json"))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Quote.json not found in demo pack"))?;
        let manifest_rel = session
            .known_paths
            .iter()
            .find(|rel| file_name_matches(rel, "DecisionPack.manifest.json"))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("DecisionPack.manifest.json not found in demo pack"))?;
        let replay_rel = session
            .known_paths
            .iter()
            .find(|rel| file_name_matches(rel, "REPLAY.md"))
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("REPLAY.md not found in demo pack"))?;

        let (_, quote_path) = resolve_session_file_path(&session, &quote_rel.replace('/', "\\"))?;
        let quote_preview = build_preview(&quote_rel, &quote_path)?;
        assert_eq!(quote_preview.kind, "json");

        let (_, manifest_path) =
            resolve_session_file_path(&session, &manifest_rel.replace('/', "\\"))?;
        let manifest_preview = build_preview(&manifest_rel, &manifest_path)?;
        assert_eq!(manifest_preview.kind, "json");

        let (_, replay_path) = resolve_session_file_path(&session, &replay_rel.replace('/', "\\"))?;
        let replay_preview = build_preview(&replay_rel, &replay_path)?;
        assert_eq!(replay_preview.kind, "markdown");

        Ok(())
    }

    #[test]
    fn known_good_pack_loads_and_verifies_when_available() -> Result<()> {
        let _guard = ENV_LOCK.lock().expect("env lock");

        let pack_candidates = [
            r"E:\_packs\_GOLDEN_LOCKED\GOLDEN-epi-rail-20260218-203121\pack.zip",
            r"E:\_packs\_GOLDEN_LOCKED\GOLDEN-epi-rail-20260218-200554\pack.zip",
            r"E:\_packs\_GOLDEN_LOCKED\GATEC-20260218-204921\pack.zip",
        ];
        let pack_path = pack_candidates
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file());
        let Some(pack_path) = pack_path else {
            return Ok(());
        };

        let epi_candidates = [
            r"E:\CupolaCore\target\release\epi-cli.exe",
            r"E:\Sanctuary\products\leo\dist\LEO\tools\epi\epi-cli.exe",
        ];
        let epi_path = epi_candidates
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file());

        let previous = std::env::var_os("EPI_CLI_PATH");
        if let Some(epi_path) = &epi_path {
            std::env::set_var("EPI_CLI_PATH", epi_path);
        }

        let loaded = load_pack_impl(&pack_path)?;
        assert!(!loaded.response.files.is_empty());
        assert!(!loaded.response.claims.is_empty());

        if epi_path.is_some() {
            assert_eq!(loaded.response.verification.status, "ok");
        }

        if let Some(value) = previous {
            std::env::set_var("EPI_CLI_PATH", value);
        } else {
            std::env::remove_var("EPI_CLI_PATH");
        }

        Ok(())
    }
}

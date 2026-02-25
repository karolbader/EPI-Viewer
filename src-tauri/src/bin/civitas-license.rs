use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

const SCHEMA_VERSION: &str = "civitas.entitlements.v1";

#[derive(Debug)]
struct KeygenArgs {
    out_dir: PathBuf,
}

#[derive(Debug)]
struct IssueArgs {
    pub_key_path: PathBuf,
    priv_key_path: PathBuf,
    out_path: PathBuf,
    customer_id: String,
    order_id: String,
    plan: String,
    expires_at: Option<String>,
    entitled_libraries: Vec<String>,
    bonus_libraries: Vec<String>,
    byol_enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct EntitlementsPayload {
    schema_version: String,
    customer_id: String,
    order_id: String,
    plan: String,
    entitled_libraries: Vec<String>,
    bonus_libraries: Vec<String>,
    issued_at: String,
    expires_at: Option<String>,
    byol_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
struct SignedEntitlements {
    schema_version: String,
    customer_id: String,
    order_id: String,
    plan: String,
    entitled_libraries: Vec<String>,
    bonus_libraries: Vec<String>,
    issued_at: String,
    expires_at: Option<String>,
    byol_enabled: bool,
    signature: String,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(|| anyhow!(usage()))?;
    let rest = args.collect::<Vec<String>>();

    match command.as_str() {
        "keygen" => run_keygen(parse_keygen_args(&rest)?),
        "issue" => run_issue(parse_issue_args(&rest)?),
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        _ => bail!("unknown command: {command}\n\n{}", usage()),
    }
}

fn run_keygen(args: KeygenArgs) -> Result<()> {
    fs::create_dir_all(&args.out_dir).with_context(|| {
        format!(
            "failed to create output directory {}",
            args.out_dir.display()
        )
    })?;

    let mut secret_bytes = [0_u8; 32];
    let mut rng = OsRng;
    rng.fill_bytes(&mut secret_bytes);
    let signing_key = SigningKey::from_bytes(&secret_bytes);
    let verifying_key = signing_key.verifying_key();

    let public_b64 = BASE64_STANDARD.encode(verifying_key.as_bytes());
    let private_b64 = BASE64_STANDARD.encode(signing_key.to_bytes());

    let public_out = args.out_dir.join("ed25519_public_key.b64");
    let private_out = args.out_dir.join("ed25519_private_key.b64");
    fs::write(&public_out, format!("{public_b64}\n"))
        .with_context(|| format!("failed to write {}", public_out.display()))?;
    fs::write(&private_out, format!("{private_b64}\n"))
        .with_context(|| format!("failed to write {}", private_out.display()))?;

    println!("Generated Ed25519 keypair.");
    println!("PUBLIC_KEY_B64={public_b64}");
    println!("public key file: {}", public_out.display());
    println!("private key file: {}", private_out.display());
    println!("Copy this PUBLIC_KEY_B64 into entitlements.rs constant: CIVITAS_LICENSE_PUBKEY_B64");
    Ok(())
}

fn run_issue(args: IssueArgs) -> Result<()> {
    let public_key_bytes = decode_32_bytes(read_trimmed(&args.pub_key_path)?, "public key")?;
    let private_key_bytes = decode_32_bytes(read_trimmed(&args.priv_key_path)?, "private key")?;

    let signing_key = SigningKey::from_bytes(&private_key_bytes);
    let expected_verifying_key = VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|err| anyhow!("invalid public key bytes ({err})"))?;
    let derived_verifying_key = signing_key.verifying_key();
    if derived_verifying_key.as_bytes() != expected_verifying_key.as_bytes() {
        bail!("provided private key does not match provided public key");
    }

    let payload = EntitlementsPayload {
        schema_version: SCHEMA_VERSION.to_string(),
        customer_id: args.customer_id,
        order_id: args.order_id,
        plan: args.plan,
        entitled_libraries: args.entitled_libraries,
        bonus_libraries: args.bonus_libraries,
        issued_at: now_rfc3339_utc(),
        expires_at: args.expires_at,
        byol_enabled: args.byol_enabled,
    };
    validate_payload(&payload)?;

    let payload_json =
        serde_json::to_string(&payload).context("failed to serialize payload before signing")?;
    let signature = signing_key.sign(payload_json.as_bytes());
    derived_verifying_key
        .verify(payload_json.as_bytes(), &signature)
        .map_err(|_| anyhow!("self-verification failed after signing"))?;
    let signature_b64 = BASE64_STANDARD.encode(signature.to_bytes());

    let signed = SignedEntitlements {
        schema_version: payload.schema_version,
        customer_id: payload.customer_id,
        order_id: payload.order_id,
        plan: payload.plan,
        entitled_libraries: payload.entitled_libraries,
        bonus_libraries: payload.bonus_libraries,
        issued_at: payload.issued_at,
        expires_at: payload.expires_at,
        byol_enabled: payload.byol_enabled,
        signature: signature_b64,
    };

    let signed_json = serde_json::to_string_pretty(&signed)
        .context("failed to serialize signed entitlements JSON")?;
    ensure_parent_dir(&args.out_path)?;
    fs::write(&args.out_path, format!("{signed_json}\n"))
        .with_context(|| format!("failed to write {}", args.out_path.display()))?;

    let public_b64 = BASE64_STANDARD.encode(derived_verifying_key.as_bytes());
    println!("Issued entitlement file: {}", args.out_path.display());
    println!("PUBLIC_KEY_B64={public_b64}");
    println!("Copy this PUBLIC_KEY_B64 into entitlements.rs constant: CIVITAS_LICENSE_PUBKEY_B64");
    Ok(())
}

fn parse_keygen_args(args: &[String]) -> Result<KeygenArgs> {
    let mut out_dir: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("missing value for --out"))?;
                out_dir = Some(PathBuf::from(value));
            }
            unknown => bail!("unknown argument for keygen: {unknown}"),
        }
        index += 1;
    }

    Ok(KeygenArgs {
        out_dir: out_dir.ok_or_else(|| anyhow!("--out is required for keygen"))?,
    })
}

fn parse_issue_args(args: &[String]) -> Result<IssueArgs> {
    let mut pub_key_path: Option<PathBuf> = None;
    let mut priv_key_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut customer_id: Option<String> = None;
    let mut order_id: Option<String> = None;
    let mut plan: Option<String> = None;
    let mut expires_at: Option<Option<String>> = None;
    let mut entitled_libraries: Option<Vec<String>> = None;
    let mut bonus_libraries = Vec::new();
    let mut byol_enabled = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--pub" => {
                index += 1;
                pub_key_path = Some(PathBuf::from(required_value(args, index, "--pub")?));
            }
            "--priv" => {
                index += 1;
                priv_key_path = Some(PathBuf::from(required_value(args, index, "--priv")?));
            }
            "--out" => {
                index += 1;
                out_path = Some(PathBuf::from(required_value(args, index, "--out")?));
            }
            "--customer" => {
                index += 1;
                customer_id = Some(required_value(args, index, "--customer")?.to_string());
            }
            "--order" => {
                index += 1;
                order_id = Some(required_value(args, index, "--order")?.to_string());
            }
            "--plan" => {
                index += 1;
                plan = Some(required_value(args, index, "--plan")?.to_string());
            }
            "--expires" => {
                index += 1;
                expires_at = Some(parse_expires(required_value(args, index, "--expires")?)?);
            }
            "--libs" => {
                index += 1;
                entitled_libraries = Some(parse_csv_list(required_value(args, index, "--libs")?));
            }
            "--bonus" => {
                index += 1;
                bonus_libraries = parse_csv_list(required_value(args, index, "--bonus")?);
            }
            "--byol" => {
                byol_enabled = true;
            }
            unknown => bail!("unknown argument for issue: {unknown}"),
        }
        index += 1;
    }

    let entitled_libraries =
        entitled_libraries.ok_or_else(|| anyhow!("--libs is required for issue"))?;
    if entitled_libraries.is_empty() {
        bail!("--libs must include at least one library id");
    }

    Ok(IssueArgs {
        pub_key_path: pub_key_path.ok_or_else(|| anyhow!("--pub is required for issue"))?,
        priv_key_path: priv_key_path.ok_or_else(|| anyhow!("--priv is required for issue"))?,
        out_path: out_path.ok_or_else(|| anyhow!("--out is required for issue"))?,
        customer_id: customer_id.ok_or_else(|| anyhow!("--customer is required for issue"))?,
        order_id: order_id.ok_or_else(|| anyhow!("--order is required for issue"))?,
        plan: plan.ok_or_else(|| anyhow!("--plan is required for issue"))?,
        expires_at: expires_at.ok_or_else(|| anyhow!("--expires is required for issue"))?,
        entitled_libraries,
        bonus_libraries,
        byol_enabled,
    })
}

fn required_value<'a>(args: &'a [String], index: usize, flag: &str) -> Result<&'a str> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing value for {flag}"))
}

fn parse_expires(value: &str) -> Result<Option<String>> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(None);
    }

    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|err| anyhow!("--expires must be RFC3339 or 'none' ({err})"))?;
    Ok(Some(
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true),
    ))
}

fn parse_csv_list(value: &str) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut list = Vec::new();
    for candidate in value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let normalized = candidate.to_string();
        if seen.insert(normalized.clone()) {
            list.push(normalized);
        }
    }
    list
}

fn decode_32_bytes(value: String, label: &str) -> Result<[u8; 32]> {
    let bytes = BASE64_STANDARD
        .decode(value)
        .map_err(|err| anyhow!("invalid base64 {label} ({err})"))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("{label} must decode to exactly 32 bytes"))
}

fn read_trimmed(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read key file {}", path.display()))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("key file is empty: {}", path.display());
    }
    Ok(trimmed.to_string())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))
}

fn now_rfc3339_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn validate_payload(payload: &EntitlementsPayload) -> Result<()> {
    if payload.schema_version != SCHEMA_VERSION {
        bail!("schema_version must be {SCHEMA_VERSION}");
    }
    if payload.customer_id.trim().is_empty() {
        bail!("customer_id cannot be empty");
    }
    if payload.order_id.trim().is_empty() {
        bail!("order_id cannot be empty");
    }
    if payload.plan.trim().is_empty() {
        bail!("plan cannot be empty");
    }
    DateTime::parse_from_rfc3339(&payload.issued_at)
        .map_err(|err| anyhow!("issued_at must be RFC3339 ({err})"))?;
    if let Some(expires_at) = &payload.expires_at {
        DateTime::parse_from_rfc3339(expires_at)
            .map_err(|err| anyhow!("expires_at must be RFC3339 ({err})"))?;
    }
    Ok(())
}

fn usage() -> String {
    [
        "Usage:",
        "  civitas-license keygen --out <DIR>",
        "  civitas-license issue --pub <PUB_B64_FILE> --priv <PRIV_B64_FILE> --out <ENTITLEMENTS_JSON> --customer <ID> --order <ID> --plan <PLAN> --expires <RFC3339|none> --libs <comma list> [--bonus <comma list>] [--byol]",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signature;

    fn sample_payload() -> EntitlementsPayload {
        EntitlementsPayload {
            schema_version: SCHEMA_VERSION.to_string(),
            customer_id: "cust_001".to_string(),
            order_id: "ord_001".to_string(),
            plan: "EU_DUO".to_string(),
            entitled_libraries: vec![
                "vendorsecurity/v1".to_string(),
                "iso27001-lite/v1".to_string(),
            ],
            bonus_libraries: vec!["soc_2".to_string()],
            issued_at: "2026-02-25T00:00:00Z".to_string(),
            expires_at: Some("2026-12-31T23:59:59Z".to_string()),
            byol_enabled: false,
        }
    }

    #[test]
    fn payload_serialization_is_stable() {
        let payload = sample_payload();
        let serialized = serde_json::to_string(&payload).expect("serialize");
        assert_eq!(
            serialized,
            r#"{"schema_version":"civitas.entitlements.v1","customer_id":"cust_001","order_id":"ord_001","plan":"EU_DUO","entitled_libraries":["vendorsecurity/v1","iso27001-lite/v1"],"bonus_libraries":["soc_2"],"issued_at":"2026-02-25T00:00:00Z","expires_at":"2026-12-31T23:59:59Z","byol_enabled":false}"#
        );
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let payload = sample_payload();
        let payload_json = serde_json::to_string(&payload).expect("serialize");
        let signing_key = SigningKey::from_bytes(&[13_u8; 32]);
        let verifying_key = signing_key.verifying_key();
        let signature = signing_key.sign(payload_json.as_bytes());
        let signature = Signature::from_slice(&signature.to_bytes()).expect("signature bytes");
        verifying_key
            .verify(payload_json.as_bytes(), &signature)
            .expect("signature should verify");
    }
}

# EPI Viewer v0

Local-only desktop viewer for inspecting EPI `pack.zip` artifacts.

## Scope

- Viewer-only. No pack generation.
- No network calls.
- Read-only file inspection.

## Prerequisites

- Rust toolchain (`rustc`, `cargo`)
- Platform prerequisites for Tauri desktop apps:
  - Windows: WebView2 runtime
  - Linux/macOS: default Tauri prerequisites for your distro/OS

## Run (Dev)

From repo root:

```powershell
.\scripts\run_dev.ps1
```

This starts the desktop viewer using a repo-local cargo target directory (`.\_target` by default), so non-admin runs avoid lock/permission issues.

Underlying cargo command:

```powershell
cargo run --manifest-path .\src-tauri\Cargo.toml
```

## Build

From repo root:

```powershell
.\scripts\build_release.ps1
```

The build script honors `CARGO_TARGET_DIR`; if not set, it defaults to `.\_target`.

## Quick Smoke

Run a deterministic golden-pack smoke capture (release build + 5 panel screenshots + local verify JSON + single smoke log):

```powershell
pwsh -File .\scripts\smoke_capture.ps1 -PackPath "<...>\pack.zip" -EpiCliPath "E:\CupolaCore\target\release\epi-cli.exe"
```

This smoke flow runs without admin by default (repo-local cargo target) and fails non-zero if verification fails, screenshots are missing, or the Night Carbon theme check fails.

## Verifier Lookup (`epi-cli`)

Verification uses `epi-cli verify "<pack.zip>" --json` if available.

Lookup order:

1. Next to viewer executable: `.\tools\epi\epi-cli(.exe)`
2. Environment variable: `EPI_CLI_PATH`
3. If neither exists: verification is disabled and UI shows `Verifier not found`

Example (PowerShell):

```powershell
$env:EPI_CLI_PATH = "E:\CupolaCore\target\release\epi-cli.exe"
.\scripts\run_dev.ps1
```

## UI Theme

The viewer uses Civitas Night Carbon as the base surface: `#0A0B10`, with neutral high-contrast text and dark panel surfaces.

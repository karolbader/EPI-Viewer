$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

$defaultTargetDir = Join-Path $repoRoot "_target"
if (-not $env:CARGO_TARGET_DIR -or [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
  $env:CARGO_TARGET_DIR = $defaultTargetDir
}

$resolvedTargetDir = [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
New-Item -ItemType Directory -Force -Path $resolvedTargetDir | Out-Null
$env:CARGO_TARGET_DIR = $resolvedTargetDir

cargo run --manifest-path ".\src-tauri\Cargo.toml"

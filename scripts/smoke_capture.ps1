param(
  [Parameter(Mandatory = $true)]
  [string]$PackPath,
  [Parameter(Mandatory = $true)]
  [string]$EpiCliPath,
  [string]$OutDir
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

function Resolve-AbsolutePath {
  param(
    [Parameter(Mandatory = $true)]
    [string]$PathValue
  )
  if ([System.IO.Path]::IsPathRooted($PathValue)) {
    return [System.IO.Path]::GetFullPath($PathValue)
  }
  return [System.IO.Path]::GetFullPath((Join-Path $repoRoot $PathValue))
}

$packAbsolute = Resolve-AbsolutePath -PathValue $PackPath
$epiCliAbsolute = Resolve-AbsolutePath -PathValue $EpiCliPath

if (-not (Test-Path -LiteralPath $packAbsolute -PathType Leaf)) {
  throw "PackPath not found: $packAbsolute"
}
if (-not (Test-Path -LiteralPath $epiCliAbsolute -PathType Leaf)) {
  throw "EpiCliPath not found: $epiCliAbsolute"
}

if ([string]::IsNullOrWhiteSpace($OutDir)) {
  $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
  $OutDir = Join-Path $repoRoot (Join-Path "_screens_smoke" $timestamp)
}
$outDirAbsolute = Resolve-AbsolutePath -PathValue $OutDir
New-Item -ItemType Directory -Force -Path $outDirAbsolute | Out-Null

$defaultTargetDir = Join-Path $repoRoot "_target"
if (-not $env:CARGO_TARGET_DIR -or [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
  $env:CARGO_TARGET_DIR = $defaultTargetDir
}
$targetDirAbsolute = [System.IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
New-Item -ItemType Directory -Force -Path $targetDirAbsolute | Out-Null
$env:CARGO_TARGET_DIR = $targetDirAbsolute

$stylesPath = Join-Path $repoRoot "src\styles.css"
$stylesRaw = Get-Content -Raw -LiteralPath $stylesPath
$themeHasNightCarbon = $stylesRaw -match "(?i)#0A0B10"

cargo build --release --manifest-path ".\src-tauri\Cargo.toml"

$viewerExePath = Join-Path $targetDirAbsolute "release\epi-viewer.exe"
if (-not (Test-Path -LiteralPath $viewerExePath -PathType Leaf)) {
  throw "Viewer executable not found after release build: $viewerExePath"
}
$viewerExePath = (Resolve-Path -LiteralPath $viewerExePath).Path

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

$env:EPI_CLI_PATH = $epiCliAbsolute

if (-not ("SmokeCaptureWin32" -as [type])) {
  Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SmokeCaptureWin32 {
  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

  [StructLayout(LayoutKind.Sequential)]
  public struct RECT {
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
  }
}
'@
}

function Wait-MainWindow {
  param(
    [Parameter(Mandatory = $true)]
    [System.Diagnostics.Process]$Process,
    [int]$TimeoutMs = 15000
  )
  $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
  while ([DateTime]::UtcNow -lt $deadline) {
    $Process.Refresh()
    if ($Process.MainWindowHandle -ne 0) {
      return $true
    }
    Start-Sleep -Milliseconds 150
  }
  return $false
}

function Capture-Window {
  param(
    [Parameter(Mandatory = $true)]
    [System.Diagnostics.Process]$Process,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath
  )

  $Process.Refresh()
  if ($Process.MainWindowHandle -eq 0) {
    throw "Cannot capture screenshot: no main window handle for PID $($Process.Id)"
  }

  $rect = New-Object SmokeCaptureWin32+RECT
  [void][SmokeCaptureWin32]::GetWindowRect($Process.MainWindowHandle, [ref]$rect)
  $width = $rect.Right - $rect.Left
  $height = $rect.Bottom - $rect.Top
  if ($width -le 0 -or $height -le 0) {
    throw "Cannot capture screenshot: invalid window bounds for PID $($Process.Id)"
  }

  $bitmap = New-Object System.Drawing.Bitmap($width, $height)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
  $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
  $graphics.Dispose()
  $bitmap.Dispose()
}

$tabs = @("overview", "claims", "drift", "decision", "files")
$panelPaths = @{}

foreach ($tab in $tabs) {
  $args = @("--pack", $packAbsolute, "--tab", $tab)
  $process = Start-Process -FilePath $viewerExePath -ArgumentList $args -PassThru
  try {
    if (-not (Wait-MainWindow -Process $process -TimeoutMs 20000)) {
      throw "Viewer window did not initialize for tab '$tab'"
    }

    Start-Sleep -Milliseconds 1200

    $panelPath = Join-Path $outDirAbsolute ("panel-{0}.png" -f $tab)
    Capture-Window -Process $process -OutputPath $panelPath

    if (-not (Test-Path -LiteralPath $panelPath -PathType Leaf)) {
      throw "Screenshot missing for tab '$tab': $panelPath"
    }
    $panelPaths[$tab] = (Resolve-Path -LiteralPath $panelPath).Path
  }
  finally {
    if (-not $process.HasExited) {
      $null = $process.CloseMainWindow()
      Start-Sleep -Milliseconds 500
    }
    if (-not $process.HasExited) {
      Stop-Process -Id $process.Id -Force
    }
  }
}

$verifyOutputPath = Join-Path $outDirAbsolute "verify.json"
$verifyOutputRaw = & $epiCliAbsolute verify $packAbsolute --json
$verifyExitCode = $LASTEXITCODE

if ($verifyOutputRaw -is [array]) {
  $verifyOutputRaw = $verifyOutputRaw -join [Environment]::NewLine
}
$verifyOutputRaw = [string]$verifyOutputRaw
Set-Content -LiteralPath $verifyOutputPath -Value $verifyOutputRaw -Encoding utf8
$verifyOutputPath = (Resolve-Path -LiteralPath $verifyOutputPath).Path

if ($verifyExitCode -ne 0) {
  throw "epi-cli verify failed with exit code $verifyExitCode"
}

$verifyJson = $verifyOutputRaw | ConvertFrom-Json
$verifyOk = $false
if ($null -ne $verifyJson.PSObject.Properties["ok"]) {
  $verifyOk = [bool]$verifyJson.ok
}
elseif (
  $null -ne $verifyJson.PSObject.Properties["status"] -and
  $null -ne $verifyJson.status -and
  $null -ne $verifyJson.status.PSObject.Properties["success"]
) {
  $verifyOk = [bool]$verifyJson.status.success
}

$packSha256 = (Get-FileHash -LiteralPath $packAbsolute -Algorithm SHA256).Hash.ToLowerInvariant()

$missingScreenshots = @()
foreach ($tab in $tabs) {
  if (-not $panelPaths.ContainsKey($tab)) {
    $missingScreenshots += $tab
  }
}

$smokeFailures = @()
if (-not $themeHasNightCarbon) {
  $smokeFailures += "src/styles.css does not contain #0A0B10"
}
if ($missingScreenshots.Count -gt 0) {
  $smokeFailures += "Missing screenshots for tabs: $($missingScreenshots -join ',')"
}
if (-not $verifyOk) {
  $smokeFailures += "epi-cli verify ok != true"
}

$verifyOkText = if ($verifyOk) { "true" } else { "false" }
$themeOkText = if ($themeHasNightCarbon) { "true" } else { "false" }
$verifyStatus = ""
if ($null -ne $verifyJson.PSObject.Properties["status"]) {
  if ($verifyJson.status -is [string]) {
    $verifyStatus = [string]$verifyJson.status
  }
  elseif ($null -ne $verifyJson.status -and $null -ne $verifyJson.status.PSObject.Properties["success"]) {
    $verifyStatus = if ([bool]$verifyJson.status.success) { "ok" } else { "fail" }
  }
}

$checkedEntries = ""
if ($null -ne $verifyJson.PSObject.Properties["checked_entries_count"]) {
  $checkedEntries = [string]$verifyJson.checked_entries_count
}
elseif ($null -ne $verifyJson.PSObject.Properties["file_hashes"] -and $null -ne $verifyJson.file_hashes) {
  $hashEntryCount = 0
  if ($verifyJson.file_hashes -is [System.Collections.IDictionary]) {
    $hashEntryCount = @($verifyJson.file_hashes.Keys).Count
  }
  else {
    $hashEntryCount = @($verifyJson.file_hashes.PSObject.Properties.Name).Count
  }
  $checkedEntries = [string]$hashEntryCount
}

$smokeLogPath = Join-Path $outDirAbsolute "SMOKE.txt"
$lines = @(
  "PACK_PATH=$packAbsolute",
  "EPI_CLI_PATH=$epiCliAbsolute",
  "VIEWER_EXE_PATH=$viewerExePath",
  "OUTDIR=$outDirAbsolute",
  "CARGO_TARGET_DIR=$targetDirAbsolute",
  "PACK_SHA256=$packSha256",
  "THEME_BASE_HEX=#0A0B10",
  "THEME_CSS_HAS_NIGHT_CARBON=$themeOkText",
  "VERIFY_JSON_PATH=$verifyOutputPath",
  "VERIFY_OK=$verifyOkText",
  "VERIFY_STATUS=$verifyStatus",
  "VERIFY_CHECKED_ENTRIES=$checkedEntries",
  "PANEL_OVERVIEW=$($panelPaths['overview'])",
  "PANEL_CLAIMS=$($panelPaths['claims'])",
  "PANEL_DRIFT=$($panelPaths['drift'])",
  "PANEL_DECISION=$($panelPaths['decision'])",
  "PANEL_FILES=$($panelPaths['files'])"
)

if ($smokeFailures.Count -gt 0) {
  $lines += "FAILURES=$($smokeFailures -join ' | ')"
}
else {
  $lines += "RESULT=PASS"
}

Set-Content -LiteralPath $smokeLogPath -Value $lines -Encoding utf8
$smokeLogPath = (Resolve-Path -LiteralPath $smokeLogPath).Path

Write-Output "SMOKE_LOG=$smokeLogPath"

if ($smokeFailures.Count -gt 0) {
  throw "Smoke failed: $($smokeFailures -join ' | ')"
}

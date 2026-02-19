param(
    [string]$EpiViewerRepo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path,
    [string]$LeoRepo,
    [string]$AegisRepo,
    [string]$CupolaRepo,
    [string]$DistRoot
)

$ErrorActionPreference = "Stop"

function Require-File {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing: $Path"
    }
}

function Require-Directory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "$Label is missing: $Path"
    }
}

function Resolve-StrictPath {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "$Label is missing: $Path"
    }
    return (Resolve-Path -LiteralPath $Path).Path
}

$epiViewerRepoResolved = Resolve-StrictPath -Path $EpiViewerRepo -Label "EPI-Viewer repository"
$productsRoot = Split-Path -Parent $epiViewerRepoResolved
$driveRoot = [System.IO.Path]::GetPathRoot($epiViewerRepoResolved)

if ([string]::IsNullOrWhiteSpace($LeoRepo)) {
    $LeoRepo = Join-Path $productsRoot "leo"
}
if ([string]::IsNullOrWhiteSpace($AegisRepo)) {
    $AegisRepo = Join-Path $productsRoot "aegis"
}
if ([string]::IsNullOrWhiteSpace($CupolaRepo)) {
    if (-not [string]::IsNullOrWhiteSpace($env:CUPOLA_REPO)) {
        $CupolaRepo = $env:CUPOLA_REPO
    }
    else {
        $CupolaRepo = Join-Path $driveRoot "CupolaCore"
    }
}
if ([string]::IsNullOrWhiteSpace($DistRoot)) {
    $DistRoot = Join-Path $productsRoot "dist"
}

$leoRepoResolved = Resolve-StrictPath -Path $LeoRepo -Label "LEO repository"
$aegisRepoResolved = Resolve-StrictPath -Path $AegisRepo -Label "AEGIS repository"
$cupolaRepoResolved = Resolve-StrictPath -Path $CupolaRepo -Label "CupolaCore repository"
New-Item -ItemType Directory -Path $DistRoot -Force | Out-Null
$distRootResolved = Resolve-StrictPath -Path $DistRoot -Label "dist output root"

$leoBuildScript = Join-Path $leoRepoResolved "scripts\build_portable.ps1"
$epiBuildScript = Join-Path $epiViewerRepoResolved "scripts\build_release.ps1"
$leoPortableRoot = Join-Path $leoRepoResolved "dist\LEO"
$viewerExe = Join-Path $epiViewerRepoResolved "_target\release\epi-viewer.exe"
$viewerPdb = Join-Path $epiViewerRepoResolved "_target\release\epi_viewer.pdb"
$stylesPath = Join-Path $epiViewerRepoResolved "src\styles.css"

Require-File -Path $leoBuildScript -Label "LEO scripts\\build_portable.ps1"
Require-File -Path $epiBuildScript -Label "EPI-Viewer scripts\\build_release.ps1"
Require-File -Path $stylesPath -Label "EPI-Viewer src\\styles.css"

Write-Host "[gate-i] Building LEO portable runtime..."
Push-Location $leoRepoResolved
try {
    & $leoBuildScript -LeoRepo $leoRepoResolved -CupolaRepo $cupolaRepoResolved -AegisRepo $aegisRepoResolved
}
finally {
    Pop-Location
}

Require-Directory -Path $leoPortableRoot -Label "LEO dist\\LEO"
Require-File -Path (Join-Path $leoPortableRoot "leo.exe") -Label "LEO portable leo.exe"
Require-File -Path (Join-Path $leoPortableRoot "tools\epi\epi-cli.exe") -Label "LEO portable epi-cli.exe"
Require-File -Path (Join-Path $leoPortableRoot "data\intake.json") -Label "LEO portable data\\intake.json"

Write-Host "[gate-i] Building EPI-Viewer release..."
Push-Location $epiViewerRepoResolved
try {
    & $epiBuildScript
}
finally {
    Pop-Location
}

Require-File -Path $viewerExe -Label "EPI-Viewer release executable"

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$bundleName = "CIVITAS-EPI-Rail-$stamp"
$bundleRoot = Join-Path $distRootResolved $bundleName
$bundleZip = Join-Path $distRootResolved ($bundleName + ".zip")
$bundleShaPath = Join-Path $distRootResolved ($bundleName + ".SHA256.txt")
$bundleShaCompatPath = Join-Path $distRootResolved "SHA256.txt"

if (Test-Path -LiteralPath $bundleRoot) {
    Remove-Item -LiteralPath $bundleRoot -Recurse -Force
}
if (Test-Path -LiteralPath $bundleZip -PathType Leaf) {
    Remove-Item -LiteralPath $bundleZip -Force
}
if (Test-Path -LiteralPath $bundleShaPath -PathType Leaf) {
    Remove-Item -LiteralPath $bundleShaPath -Force
}
if (Test-Path -LiteralPath $bundleShaCompatPath -PathType Leaf) {
    Remove-Item -LiteralPath $bundleShaCompatPath -Force
}

New-Item -ItemType Directory -Path $bundleRoot -Force | Out-Null

Write-Host "[gate-i] Staging bundle folder: $bundleRoot"
Copy-Item -LiteralPath $leoPortableRoot -Destination (Join-Path $bundleRoot "LEO") -Recurse -Force

$viewerBundleRoot = Join-Path $bundleRoot "EPI-Viewer"
New-Item -ItemType Directory -Path $viewerBundleRoot -Force | Out-Null
Copy-Item -LiteralPath $viewerExe -Destination (Join-Path $viewerBundleRoot "epi-viewer.exe") -Force
if (Test-Path -LiteralPath $viewerPdb -PathType Leaf) {
    Copy-Item -LiteralPath $viewerPdb -Destination (Join-Path $viewerBundleRoot "epi_viewer.pdb") -Force
}

$themeBundleRoot = Join-Path $viewerBundleRoot "theme"
New-Item -ItemType Directory -Path $themeBundleRoot -Force | Out-Null
Copy-Item -LiteralPath $stylesPath -Destination (Join-Path $themeBundleRoot "styles.css") -Force

$cupolaProxyRoot = Join-Path $bundleRoot "LEO\tools\cupola-proxy"
New-Item -ItemType Directory -Path (Join-Path $cupolaProxyRoot "cupola-cli\src") -Force | Out-Null

Set-Content -LiteralPath (Join-Path $cupolaProxyRoot "Cargo.toml") -Encoding utf8 -Value @'
[workspace]
members = ["cupola-cli"]
resolver = "2"
'@

Set-Content -LiteralPath (Join-Path $cupolaProxyRoot "cupola-cli\Cargo.toml") -Encoding utf8 -Value @'
[package]
name = "cupola-cli"
version = "0.0.0"
edition = "2021"
'@

Set-Content -LiteralPath (Join-Path $cupolaProxyRoot "cupola-cli\src\main.rs") -Encoding utf8 -Value @'
use std::env;
use std::process::{Command, exit};

fn main() {
    let target = match env::var("LEO_CUPOLA_EXE") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("LEO_CUPOLA_EXE is not set");
            exit(1);
        }
    };

    let status = match Command::new(target).args(env::args().skip(1)).status() {
        Ok(value) => value,
        Err(err) => {
            eprintln!("failed to execute bundled cupola-cli: {err}");
            exit(1);
        }
    };

    match status.code() {
        Some(code) => exit(code),
        None => exit(1),
    }
}
'@

Write-Host "[gate-i] Building bundled cupola proxy workspace..."
Push-Location $cupolaProxyRoot
try {
    $targetWasSet = Test-Path Env:CARGO_TARGET_DIR
    $originalTargetDir = $env:CARGO_TARGET_DIR
    $env:CARGO_TARGET_DIR = (Join-Path $cupolaProxyRoot "target")
    cargo build
    cargo build --release
}
finally {
    if ($targetWasSet) {
        $env:CARGO_TARGET_DIR = $originalTargetDir
    }
    else {
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    }
    Pop-Location
}

Require-File -Path (Join-Path $cupolaProxyRoot "target\release\cupola-cli.exe") -Label "cupola proxy release executable"
Require-File -Path (Join-Path $cupolaProxyRoot "target\debug\cupola-cli.exe") -Label "cupola proxy debug executable"

$demoVaultRoot = Join-Path $bundleRoot "demo-vault"
New-Item -ItemType Directory -Path (Join-Path $demoVaultRoot "notes") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $demoVaultRoot "evidence") -Force | Out-Null
Set-Content -LiteralPath (Join-Path $demoVaultRoot "README.md") -Encoding utf8 -Value @'
# Demo Vault

Static sample vault content used by `run_smoke.ps1`.
The smoke output is always written to `%TEMP%\civitas-epi-smoke\run-<timestamp>`.
'@
Set-Content -LiteralPath (Join-Path $demoVaultRoot "notes\summary.txt") -Encoding utf8 -Value @'
control: MFA enabled for privileged users
control: quarterly access review completed
owner: civitas-rail
'@
Set-Content -LiteralPath (Join-Path $demoVaultRoot "evidence\controls.json") -Encoding utf8 -Value @'
{
  "controls": [
    { "id": "AC-01", "status": "implemented", "owner": "security" },
    { "id": "AC-02", "status": "monitoring", "owner": "platform" }
  ]
}
'@

$runSmokePath = Join-Path $bundleRoot "run_smoke.ps1"
$runSmokeScript = @'
$ErrorActionPreference = "Stop"

function Require-File {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label is missing: $Path"
    }
}

function Require-Directory {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        throw "$Label is missing: $Path"
    }
}

function Invoke-NativeLogged {
    param(
        [Parameter(Mandatory = $true)][string]$Exe,
        [Parameter(Mandatory = $true)][string[]]$Args,
        [Parameter(Mandatory = $true)][string]$StepName,
        [Parameter(Mandatory = $true)][string]$LogsDir,
        [switch]$CaptureOutput
    )

    $stdoutPath = Join-Path $LogsDir ("{0}.stdout.log" -f $StepName)
    $stderrPath = Join-Path $LogsDir ("{0}.stderr.log" -f $StepName)

    $proc = Start-Process `
        -FilePath $Exe `
        -ArgumentList $Args `
        -NoNewWindow `
        -PassThru `
        -Wait `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath

    if ($proc.ExitCode -ne 0) {
        $stderrPreview = ""
        if (Test-Path -LiteralPath $stderrPath -PathType Leaf) {
            $stderrPreview = (Get-Content -LiteralPath $stderrPath -TotalCount 25) -join " "
        }
        throw "$StepName failed with exit code $($proc.ExitCode). stderr: $stderrPreview"
    }

    if ($CaptureOutput) {
        if (Test-Path -LiteralPath $stdoutPath -PathType Leaf) {
            return Get-Content -Raw -LiteralPath $stdoutPath
        }
        return ""
    }

    return $null
}

function Get-VerifyOk {
    param([Parameter(Mandatory = $true)][object]$VerifyObject)

    if ($null -ne $VerifyObject.PSObject.Properties["ok"]) {
        return [bool]$VerifyObject.ok
    }
    if (
        $null -ne $VerifyObject.PSObject.Properties["status"] -and
        $VerifyObject.status -isnot [string] -and
        $null -ne $VerifyObject.status.PSObject.Properties["success"]
    ) {
        return [bool]$VerifyObject.status.success
    }
    return $false
}

function Test-NightCarbonImage {
    param([Parameter(Mandatory = $true)][string]$ImagePath)

    $bitmap = New-Object System.Drawing.Bitmap($ImagePath)
    try {
        $stepX = [Math]::Max([int]($bitmap.Width / 36), 1)
        $stepY = [Math]::Max([int]($bitmap.Height / 24), 1)
        $darkCount = 0
        $sampleCount = 0

        for ($x = 0; $x -lt $bitmap.Width; $x += $stepX) {
            for ($y = 0; $y -lt $bitmap.Height; $y += $stepY) {
                $pixel = $bitmap.GetPixel($x, $y)
                if ($pixel.R -le 42 -and $pixel.G -le 48 -and $pixel.B -le 62) {
                    $darkCount += 1
                }
                $sampleCount += 1
            }
        }

        if ($sampleCount -eq 0) {
            return $false
        }
        $darkRatio = [double]$darkCount / [double]$sampleCount
        return ($darkRatio -ge 0.15)
    }
    finally {
        $bitmap.Dispose()
    }
}

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

if (-not ("SmokeCaptureWin32" -as [type])) {
    Add-Type @"
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
"@
}

function Wait-MainWindow {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process,
        [int]$TimeoutMs = 20000
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
        [Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process,
        [Parameter(Mandatory = $true)][string]$OutputPath
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

try {
    $bundleRoot = (Resolve-Path -LiteralPath $PSScriptRoot).Path
    $leoRoot = Join-Path $bundleRoot "LEO"
    $viewerRoot = Join-Path $bundleRoot "EPI-Viewer"
    $demoVaultPath = Join-Path $bundleRoot "demo-vault"

    Require-Directory -Path $leoRoot -Label "LEO folder"
    Require-Directory -Path $viewerRoot -Label "EPI-Viewer folder"
    Require-Directory -Path $demoVaultPath -Label "demo vault folder"

    $leoExe = Join-Path $leoRoot "leo.exe"
    $epiExe = Join-Path $leoRoot "tools\epi\epi-cli.exe"
    $cupolaExe = Join-Path $leoRoot "tools\cupola\cupola-cli.exe"
    $cupolaProxyExe = Join-Path $leoRoot "tools\cupola-proxy\target\release\cupola-cli.exe"
    $aegisExe = Join-Path $leoRoot "tools\aegis\aegis.exe"
    $intakePath = Join-Path $leoRoot "data\intake.json"
    $viewerExe = Join-Path $viewerRoot "epi-viewer.exe"
    $themeCss = Join-Path $viewerRoot "theme\styles.css"

    Require-File -Path $leoExe -Label "LEO executable"
    Require-File -Path $epiExe -Label "Bundled epi-cli"
    Require-File -Path $cupolaExe -Label "Bundled cupola-cli"
    Require-File -Path $cupolaProxyExe -Label "Bundled cupola proxy executable"
    Require-File -Path $aegisExe -Label "Bundled aegis executable"
    Require-File -Path $intakePath -Label "Bundled intake.json"
    Require-File -Path $viewerExe -Label "EPI-Viewer executable"
    Require-File -Path $themeCss -Label "Night Carbon theme source"

    $themeMarker = "THEME_NIGHT_CARBON_0A0B10"
    $themeRaw = Get-Content -Raw -LiteralPath $themeCss
    if (-not $themeRaw.Contains($themeMarker)) {
        throw "Night Carbon marker not found in bundled theme file: $themeCss"
    }

    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $smokeRoot = Join-Path $env:TEMP "civitas-epi-smoke"
    $outDir = Join-Path $smokeRoot ("run-" + $stamp)
    $screensDir = Join-Path $outDir "_screens"
    $logsDir = Join-Path $outDir "_logs"
    $appDataDir = Join-Path $outDir "_appdata"

    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
    New-Item -ItemType Directory -Path $screensDir -Force | Out-Null
    New-Item -ItemType Directory -Path $logsDir -Force | Out-Null
    New-Item -ItemType Directory -Path $appDataDir -Force | Out-Null

    $doctorOutput = Invoke-NativeLogged `
        -Exe $leoExe `
        -Args @("doctor") `
        -StepName "step-01-doctor" `
        -LogsDir $logsDir `
        -CaptureOutput

    if (-not ([string]$doctorOutput -match "doctor:\s+OK")) {
        throw "leo doctor did not report doctor: OK"
    }

    $appDataWasSet = Test-Path Env:APPDATA
    $originalAppData = $env:APPDATA
    $cupolaExeWasSet = Test-Path Env:LEO_CUPOLA_EXE
    $originalCupolaExe = $env:LEO_CUPOLA_EXE
    $env:APPDATA = $appDataDir
    $env:LEO_CUPOLA_EXE = $cupolaExe

    try {
        Invoke-NativeLogged `
            -Exe $leoExe `
            -Args @(
                "run",
                "--vault", $demoVaultPath,
                "--intake", $intakePath,
                "--out", $outDir,
                "--cupola-bin", $cupolaProxyExe,
                "--aegis-bin", $aegisExe,
                "--epi-bin", $epiExe
            ) `
            -StepName "step-02-run" `
            -LogsDir $logsDir | Out-Null
    }
    finally {
        if ($cupolaExeWasSet) {
            $env:LEO_CUPOLA_EXE = $originalCupolaExe
        }
        else {
            Remove-Item Env:LEO_CUPOLA_EXE -ErrorAction SilentlyContinue
        }

        if ($appDataWasSet) {
            $env:APPDATA = $originalAppData
        }
        else {
            Remove-Item Env:APPDATA -ErrorAction SilentlyContinue
        }
    }

    $packZip = Join-Path $outDir "pack.zip"
    Require-File -Path $packZip -Label "LEO output pack.zip"

    $verifyRaw = Invoke-NativeLogged `
        -Exe $epiExe `
        -Args @("verify", "--json", $packZip) `
        -StepName "step-03-verify" `
        -LogsDir $logsDir `
        -CaptureOutput

    if ([string]::IsNullOrWhiteSpace($verifyRaw)) {
        throw "epi-cli verify returned empty output"
    }

    $verifyJsonPath = Join-Path $outDir "verify.json"
    Set-Content -LiteralPath $verifyJsonPath -Value $verifyRaw -Encoding utf8

    $verifyObject = $verifyRaw | ConvertFrom-Json
    $verifyOk = Get-VerifyOk -VerifyObject $verifyObject
    if (-not $verifyOk) {
        throw "verify.ok is not true in verify.json"
    }

    $tabs = @("overview", "claims", "drift", "decision", "files")
    foreach ($tab in $tabs) {
        $panelPath = Join-Path $screensDir ("panel-{0}.png" -f $tab)
        $captureLogPath = Join-Path $logsDir ("step-04-capture-{0}.log" -f $tab)

        $process = Start-Process -FilePath $viewerExe -ArgumentList @("--pack", $packZip, "--tab", $tab) -PassThru
        try {
            if (-not (Wait-MainWindow -Process $process -TimeoutMs 20000)) {
                throw "Viewer window did not initialize for tab '$tab'"
            }
            Start-Sleep -Milliseconds 1200
            Capture-Window -Process $process -OutputPath $panelPath

            if (-not (Test-Path -LiteralPath $panelPath -PathType Leaf)) {
                throw "Screenshot missing for tab '$tab': $panelPath"
            }
            $panelInfo = Get-Item -LiteralPath $panelPath
            if ($panelInfo.Length -le 0) {
                throw "Screenshot is empty for tab '$tab': $panelPath"
            }
            if (-not (Test-NightCarbonImage -ImagePath $panelPath)) {
                throw "Night Carbon dark palette check failed for tab '$tab': $panelPath"
            }

            $logLine = "tab=$tab`npath=$($panelInfo.FullName)`nbytes=$($panelInfo.Length)"
            Set-Content -LiteralPath $captureLogPath -Value $logLine -Encoding utf8
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

    $screenshotPaths = Get-ChildItem -LiteralPath $screensDir -Filter "panel-*.png" -File |
        Sort-Object Name |
        ForEach-Object { $_.FullName }

    if ($screenshotPaths.Count -ne $tabs.Count) {
        throw "Expected $($tabs.Count) screenshots, found $($screenshotPaths.Count)"
    }

    $packSha256 = (Get-FileHash -LiteralPath $packZip -Algorithm SHA256).Hash.ToLowerInvariant()
    $smokePath = Join-Path $outDir "SMOKE.txt"

    $smokeLines = @(
        "BUNDLE_ROOT=$bundleRoot",
        "LEO_ROOT=$leoRoot",
        "VIEWER_ROOT=$viewerRoot",
        "VAULT_PATH=$demoVaultPath",
        "OUT_DIR=$outDir",
        "PACK_PATH=$packZip",
        "PACK_SHA256=$packSha256",
        "VERIFY_JSON_PATH=$verifyJsonPath",
        "VERIFY_OK=true",
        "THEME_MARKER=$themeMarker"
    )

    $orderedScreens = $screenshotPaths | Sort-Object
    for ($i = 0; $i -lt $orderedScreens.Count; $i++) {
        $smokeLines += ("SCREENSHOT_{0}={1}" -f ($i + 1), $orderedScreens[$i])
    }
    $smokeLines += "RESULT=PASS"

    Set-Content -LiteralPath $smokePath -Value $smokeLines -Encoding utf8

    Write-Output ("OUT_DIR=" + $outDir)
    Write-Output ("PACK_ZIP=" + $packZip)
    Write-Output ("VERIFY_JSON=" + $verifyJsonPath)
    foreach ($screenshotPath in $orderedScreens) {
        Write-Output ("SCREENSHOT=" + $screenshotPath)
    }
    Write-Output ("SMOKE_LOG=" + $smokePath)
    Write-Output "PASS"
    exit 0
}
catch {
    Write-Output ("FAIL " + $_.Exception.Message)
    exit 1
}
'@
Set-Content -LiteralPath $runSmokePath -Value $runSmokeScript -Encoding utf8

$readmePath = Join-Path $bundleRoot "README.md"
Set-Content -LiteralPath $readmePath -Encoding utf8 -Value @'
# CIVITAS EPI Rail Bundle

## Quick Start

Run from this folder:

```powershell
pwsh -File .\run_smoke.ps1
```

## Artifact Summary

- `pack.zip`: EPI rail output pack generated by `leo.exe run`.
- `verify.json`: Tamper/contract verification from `epi-cli verify --json`.
- `_screens\panel-*.png`: Five tab screenshots from EPI-Viewer (`overview`, `claims`, `drift`, `decision`, `files`).
- `SMOKE.txt`: Deterministic PASS/FAIL summary with absolute paths and `pack.zip` SHA256.
'@

Write-Host "[gate-i] Creating zip: $bundleZip"
Compress-Archive -LiteralPath $bundleRoot -DestinationPath $bundleZip -CompressionLevel Optimal

$zipHash = (Get-FileHash -LiteralPath $bundleZip -Algorithm SHA256).Hash.ToLowerInvariant()
$shaLines = @(
    "ZIP_PATH=$bundleZip",
    "SHA256=$zipHash"
)
Set-Content -LiteralPath $bundleShaPath -Encoding utf8 -Value $shaLines
Set-Content -LiteralPath $bundleShaCompatPath -Encoding utf8 -Value $shaLines

Write-Output ("BUNDLE_DIR=" + $bundleRoot)
Write-Output ("BUNDLE_ZIP=" + $bundleZip)
Write-Output ("SHA256_FILE=" + $bundleShaPath)
Write-Output ("SHA256_TXT=" + $bundleShaCompatPath)
Write-Output ("ZIP_SHA256=" + $zipHash)

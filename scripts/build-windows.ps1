<#
.SYNOPSIS
    Build the Windows x64 portable package for texas-holdem-rs.

.DESCRIPTION
    Runs cargo build --release for poker-app, stages the executable plus runtime
    assets into dist/texas-holdem-rs-windows-x64/, and optionally creates a
    portable zip. Must be run on Windows x64 with the MSVC toolchain (no Linux
    cross compilation is attempted).

.PARAMETER Configuration
    Cargo profile to build. Default: release.

.PARAMETER SkipBuild
    Reuse an existing target binary and only rebuild the dist package.

.PARAMETER NoZip
    Do not create the portable zip.

.PARAMETER Clean
    Run cargo clean -p poker-app before building.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-windows.ps1

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-windows.ps1 -SkipBuild -NoZip
#>
[CmdletBinding()]
param(
    [string]$Configuration = "release",
    [switch]$SkipBuild,
    [switch]$NoZip,
    [switch]$Clean
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Detail([string]$Message) {
    Write-Host "    $Message" -ForegroundColor DarkGray
}

function Fail([string]$Message) {
    Write-Host ""
    Write-Host "ERROR: $Message" -ForegroundColor Red
    Write-Host "See dist\build-windows.log for the full log." -ForegroundColor Red
    exit 1
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$DistDir = Join-Path $RepoRoot "dist"
$StageName = "texas-holdem-rs-windows-x64"
$StageDir = Join-Path $DistDir $StageName
$ZipPath = Join-Path $DistDir "$StageName.zip"
$LogPath = Join-Path $DistDir "build-windows.log"
$TargetTriple = "x86_64-pc-windows-msvc"

New-Item -ItemType Directory -Path $DistDir -Force | Out-Null
$TranscriptStarted = $false
try {
    Start-Transcript -Path $LogPath -Force | Out-Null
    $TranscriptStarted = $true
} catch {
    Write-Host "WARNING: could not start transcript ($($_.Exception.Message)); continuing without log file." -ForegroundColor Yellow
}

Write-Step "texas-holdem-rs Windows portable build"
Write-Detail "repo root     : $RepoRoot"
Write-Detail "configuration : $Configuration"
Write-Detail "target triple : $TargetTriple"
Write-Detail "output        : $StageDir"
Write-Detail "timestamp     : $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"

try {
    Write-Step "Checking the Rust toolchain"
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Fail "cargo was not found on PATH. Install rustup (https://rustup.rs) and reopen the terminal."
    }
    if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
        Fail "rustc was not found on PATH. Install rustup (https://rustup.rs) and reopen the terminal."
    }
    $cargoVersion = (& cargo --version) -join " "
    $rustcVersion = (& rustc -Vv) -join [Environment]::NewLine
    Write-Detail $cargoVersion
    Write-Detail (($rustcVersion -split [Environment]::NewLine)[0])
    if ($rustcVersion -notmatch "host:\s+x86_64-pc-windows-msvc") {
        Write-Host "WARNING: rustc host is not x86_64-pc-windows-msvc. The produced binary may not be a native MSVC build." -ForegroundColor Yellow
    }
    if ($rustcVersion -match "rustc 1\.(\d+)\.") {
        $minor = [int]$Matches[1]
        if ($minor -lt 95) {
            Fail "Rust 1.95+ is required (eframe 0.37). Run: rustup update stable"
        }
    }
    $targets = (& rustup target list --installed) -join " "
    if ($targets -notmatch [Regex]::Escape($TargetTriple)) {
        Fail "target $TargetTriple is not installed. Run: rustup target add $TargetTriple"
    }

    Push-Location $RepoRoot
    try {
        if ($Clean) {
            Write-Step "Cleaning the poker-app build artifacts"
            & cargo clean -p poker-app
            if ($LASTEXITCODE -ne 0) { Fail "cargo clean failed (exit $LASTEXITCODE)" }
        }

        if (-not $SkipBuild) {
            Write-Step "Building poker-app ($Configuration, $TargetTriple)"
            & cargo build --$Configuration -p poker-app --target $TargetTriple
            if ($LASTEXITCODE -ne 0) { Fail "cargo build failed (exit $LASTEXITCODE)" }
        } else {
            Write-Step "Skipping cargo build (-SkipBuild)"
        }
    } finally {
        Pop-Location
    }

    $ExePath = Join-Path $RepoRoot "target\$TargetTriple\$Configuration\poker-app.exe"
    if (-not (Test-Path $ExePath)) {
        Fail "expected executable not found: $ExePath (run without -SkipBuild first)"
    }
    Write-Detail "executable: $ExePath ($([math]::Round((Get-Item $ExePath).Length / 1MB, 2)) MB)"

    Write-Step "Staging the portable directory"
    if (Test-Path $StageDir) {
        Remove-Item -Path $StageDir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $StageDir -Force | Out-Null
    Copy-Item -Path $ExePath -Destination (Join-Path $StageDir "poker-app.exe") -Force
    Copy-Item -Path (Join-Path $RepoRoot "assets") -Destination (Join-Path $StageDir "assets") -Recurse -Force

    $smokeDoc = Join-Path $RepoRoot "docs\windows-smoke.md"
    if (Test-Path $smokeDoc) {
        Copy-Item -Path $smokeDoc -Destination (Join-Path $StageDir "WINDOWS-SMOKE.md") -Force
    }

    $readmeLines = @(
        "德州扑克 Rust 重制版 - Windows x64 便携版",
        "==============================================",
        "",
        "运行：双击 poker-app.exe（首次启动可能被 SmartScreen 拦截：更多信息 -> 仍要运行）",
        "低内存启动：双击 run-low-memory.cmd（等价于 poker-app.exe --low-memory）",
        "",
        "目录说明：",
        "  poker-app.exe       主程序",
        "  assets/music/       背景音乐（mp3，运行时从 exe 同目录加载）",
        "  assets/icons/       图标资源",
        "  WINDOWS-SMOKE.md    Windows 冒烟清单（测试后请把结果反馈给开发）",
        "",
        "配置与存档：%APPDATA%\\texas-holdem-rs\\settings.json",
        "问题反馈：请附带配置文件和发生问题时的截图/录屏。",
        "",
        "版本：由 scripts/build-windows.ps1 生成"
    )
    Set-Content -Path (Join-Path $StageDir "README.txt") -Value $readmeLines -Encoding UTF8

    $lowMemoryCmd = @(
        "@echo off",
        "rem 低内存模式占位：--low-memory 关闭动画/预加载等（Phase 7 完善）",
        "poker-app.exe --low-memory %*"
    )
    Set-Content -Path (Join-Path $StageDir "run-low-memory.cmd") -Value $lowMemoryCmd -Encoding ASCII

    Write-Detail "staged files:"
    Get-ChildItem -Path $StageDir -Recurse -File | ForEach-Object {
        Write-Detail ("  {0}  ({1:N0} bytes)" -f $_.FullName.Substring($StageDir.Length + 1), $_.Length)
    }

    if (-not $NoZip) {
        Write-Step "Creating the portable zip"
        if (Test-Path $ZipPath) {
            Remove-Item -Path $ZipPath -Force
        }
        Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -CompressionLevel Optimal
        if (-not (Test-Path $ZipPath)) {
            Fail "zip creation failed: $ZipPath"
        }
        Write-Detail "zip: $ZipPath ($([math]::Round((Get-Item $ZipPath).Length / 1MB, 2)) MB)"
    }

    Write-Step "Done"
    Write-Host "Portable directory : $StageDir" -ForegroundColor Green
    if (-not $NoZip) {
        Write-Host "Portable zip       : $ZipPath" -ForegroundColor Green
    }
    Write-Host "Build log          : $LogPath" -ForegroundColor Green
    exit 0
} catch {
    Write-Host ""
    Write-Host "ERROR: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host $_.ScriptStackTrace -ForegroundColor DarkGray
    exit 1
} finally {
    if ($TranscriptStarted) {
        try { Stop-Transcript | Out-Null } catch { }
    }
}

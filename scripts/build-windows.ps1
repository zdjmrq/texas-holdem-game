<#
.SYNOPSIS
    Build one native Windows portable directory. No zip archives are generated.
.PARAMETER SkipBuild
    Stage an existing native build.
.PARAMETER NoZip
    Accepted for compatibility; builds always omit zip archives.
.PARAMETER Clean
    Move this project's previous target directory to the Windows Recycle Bin first.
#>
[CmdletBinding()]
param(
    [ValidateSet('release', 'dev', 'debug')]
    [string]$Configuration = 'release',
    [switch]$SkipBuild,
    [switch]$NoZip,
    [switch]$Clean
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$DistDir = Join-Path $RepoRoot 'dist'
$StageDir = Join-Path $DistDir 'texas-holdem-rs-current'
$TargetDir = Join-Path $RepoRoot 'target'
$Profile = if ($Configuration -eq 'debug') { 'dev' } else { $Configuration }
$ProfileDir = if ($Profile -eq 'dev') { 'debug' } else { $Profile }

function Move-BuildOutputToRecycleBin([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $Resolved = (Resolve-Path -LiteralPath $Path).Path
    if (-not $Resolved.StartsWith($RepoRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to recycle a path outside this repository: $Resolved"
    }
    $Item = Get-Item -LiteralPath $Resolved -Force
    if ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing to follow a linked build directory: $Resolved"
    }
    $Running = Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.Path -and $_.Path.StartsWith($Resolved + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)
    }
    if ($Running) { throw "Close the application using $Resolved before replacing its build output." }
    if ($Item.PSIsContainer -and (Get-ChildItem -LiteralPath $Resolved -Recurse -Force | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint })) {
        throw "Refusing to recycle build output containing links: $Resolved"
    }
    Add-Type -AssemblyName Microsoft.VisualBasic
    if ($Item.PSIsContainer) {
        [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($Resolved, 'OnlyErrorDialogs', 'SendToRecycleBin')
    } else {
        [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($Resolved, 'OnlyErrorDialogs', 'SendToRecycleBin')
    }
}

if ($Clean -and $SkipBuild) { throw '-Clean and -SkipBuild cannot be combined.' }
if ($Clean) { Move-BuildOutputToRecycleBin $TargetDir }

# Discover the installed MSVC environment, including custom installation drives.
if (-not $SkipBuild -and -not $env:VSCMD_VER) {
    $VsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $VsWhere) {
        $VsRoot = & $VsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($VsRoot) {
            $DevCmd = Join-Path $VsRoot 'Common7\Tools\VsDevCmd.bat'
            & $env:ComSpec /d /c "`"$DevCmd`" -arch=x64 -host_arch=x64 >nul && set" | ForEach-Object {
                if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process') }
            }
            if ($LASTEXITCODE -ne 0) { throw 'Could not initialize MSVC.' }
        }
    }
}

Push-Location -LiteralPath $RepoRoot
try {
    if (-not $SkipBuild) {
        $Version = (& rustc -Vv) -join "`n"
        if ($LASTEXITCODE -ne 0 -or $Version -notmatch 'host: x86_64-pc-windows-msvc') { throw 'Native Windows x64 MSVC Rust toolchain required.' }
        # Native host output avoids compiling dependencies into two target trees.
        & cargo build --profile $Profile -p poker-app --locked
        if ($LASTEXITCODE -ne 0) { throw "Cargo build failed ($LASTEXITCODE)." }
    }
    $ExePath = Join-Path $TargetDir "$ProfileDir\poker-app.exe"
    if (-not (Test-Path -LiteralPath $ExePath)) { throw "Executable not found: $ExePath" }
    # Replace the previous portable output recoverably; keep only one current build.
    Move-BuildOutputToRecycleBin $StageDir
    New-Item -ItemType Directory -Path $StageDir -Force | Out-Null
    Copy-Item -LiteralPath $ExePath -Destination (Join-Path $StageDir 'texas-holdem-rs.exe')
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'assets') -Destination (Join-Path $StageDir 'assets') -Recurse
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'docs\windows-smoke.md') -Destination (Join-Path $StageDir 'WINDOWS-SMOKE.md')
    Copy-Item -LiteralPath (Join-Path $RepoRoot 'docs\RESTORATION_REPORT.md') -Destination (Join-Path $StageDir 'RESTORATION_REPORT.md')
    @'
德州扑克 Rust 重制版 - Windows x64

双击 texas-holdem-rs.exe 开始。正常开局使用系统随机数。
不要给日常启动添加 --scenario 或 --seed；这两个参数用于复现测试。
保留同目录 assets，音乐和字体从这里读取。
设置：%APPDATA%\texas-holdem-rs\settings.json
此目录为唯一当前便携版，不生成 zip。
'@ | Set-Content -LiteralPath (Join-Path $StageDir 'README.txt') -Encoding UTF8
    Write-Host "Portable directory: $StageDir"
} finally {
    Pop-Location
}

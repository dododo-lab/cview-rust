[CmdletBinding()]
param(
    [string]$Root = 'C:\CViewBuild',
    [string]$SourcePath,
    [string]$OutputPath,
    [string]$LogPath,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
trap {
    if ($LogPath) { [IO.File]::AppendAllText($LogPath, (($_ | Out-String) + "`r`n"), [Text.UTF8Encoding]::new($false)) }
    throw $_
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$env:CARGO_TARGET_DIR = Join-Path $Root 'target'
$env:CI = 'true'
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
# Both MSVC targets must embed the CRT; installers cannot rely on a separately
# installed Visual C++ redistributable. The Mac audits all imported DLLs later.
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
$env:CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'

function Invoke-Native([string]$Exe, [string[]]$Arguments) {
    if ($LogPath) {
        & (Join-Path $Root 'python\python.exe') (Join-Path $PSScriptRoot 'build_runner.py') --native-log $LogPath -- $Exe @Arguments
    } else {
        & $Exe @Arguments
    }
    if ($LASTEXITCODE -ne 0) { throw "$Exe failed (exit $LASTEXITCODE)" }
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (!(Test-Path $vswhere)) { throw 'Visual Studio Build Tools is missing; run Install-BuildServer.ps1.' }
$install = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath
if (!$install) { throw 'Both x64 and ARM64 C++ build tools must be installed.' }
$developerShell = Join-Path $install 'Common7\Tools\Launch-VsDevShell.ps1'
if (!(Test-Path $developerShell)) { throw 'Visual Studio developer shell is missing.' }
foreach ($exe in @('cargo', 'rustup')) { if (!(Get-Command $exe -ErrorAction SilentlyContinue)) { throw "$exe is missing" } }
$installed = & rustup target list --installed
if ($LASTEXITCODE -ne 0) { throw 'rustup target list failed' }
foreach ($target in @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')) {
    if ($installed -notcontains $target) { throw "Missing Rust target: $target" }
}
Invoke-Native 'cargo' @('tauri', '--version')
if ($CheckOnly) { Write-Output 'Ready: MSVC x64 + ARM64, Rust, Tauri CLI'; exit 0 }
if (!$SourcePath -or !$OutputPath) { throw 'SourcePath and OutputPath are required.' }
$SourcePath = (Resolve-Path $SourcePath).Path
$version = (Get-Content (Join-Path $SourcePath 'crates\cview-app\tauri.conf.json') -Raw | ConvertFrom-Json).version
New-Item -ItemType Directory -Force $OutputPath | Out-Null
Push-Location $SourcePath
try {
    & $developerShell -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    Invoke-Native 'cargo' @('fmt', '--all', '--check')
    Invoke-Native 'cargo' @('clippy', '--workspace', '--all-targets', '--locked', '--target', 'x86_64-pc-windows-msvc', '--', '-D', 'warnings')
    Invoke-Native 'cargo' @('test', '--workspace', '--locked', '--target', 'x86_64-pc-windows-msvc')
    foreach ($entry in @(@{Arch='x64'; Target='x86_64-pc-windows-msvc'; VcArch='amd64'},
                         @{Arch='arm64'; Target='aarch64-pc-windows-msvc'; VcArch='arm64'})) {
        & $developerShell -Arch $entry.VcArch -HostArch amd64 -SkipAutomaticLocation
        if ($entry.Arch -eq 'arm64') {
            Invoke-Native 'cargo' @('clippy', '-p', 'cview-app', '--all-targets', '--locked', '--target', $entry.Target, '--', '-D', 'warnings')
        }
        $bundle = Join-Path $env:CARGO_TARGET_DIR "$($entry.Target)\release\bundle\nsis"
        if (Test-Path $bundle) { Get-ChildItem $bundle -Filter '*.exe' | Remove-Item -Force }
        Push-Location (Join-Path $SourcePath 'crates\cview-app')
        try {
            Invoke-Native 'cargo' @('tauri', 'build', '--ci', '--target', $entry.Target, '--bundles', 'nsis', '--', '--locked')
        } finally { Pop-Location }
        $installers = @(Get-ChildItem $bundle -Filter '*.exe')
        if ($installers.Count -ne 1) { throw "Expected exactly one new $($entry.Arch) NSIS installer" }
        $destination = Join-Path $OutputPath "CView_${version}_$($entry.Arch)-setup.exe"
        if (Test-Path $destination) { throw "Artifact already exists: $destination" }
        Copy-Item $installers[0].FullName $destination
        Get-FileHash $destination -Algorithm SHA256 | Format-List
    }
} finally { Pop-Location }

# Run once in an elevated Windows PowerShell on the coordinator.
[CmdletBinding()]
param(
    [string]$Root = 'C:\CViewBuild',
    [Parameter(Mandatory=$true)][string]$ClientPublicKey,
    [Parameter(Mandatory=$true)][string]$ClientAddress
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script in an elevated Windows PowerShell.'
}
# The forced-command line must not contain spaces, quotes, or shell operators.
if ($Root -notmatch '^[A-Za-z]:\\[A-Za-z0-9_\\-]+$') { throw 'Use a simple absolute Root without spaces.' }
if ($ClientPublicKey -notmatch '^ssh-ed25519 [A-Za-z0-9+/=]+(?: [A-Za-z0-9_.@-]+)?$') { throw 'Invalid Ed25519 public key' }
if ($ClientAddress -notmatch '^\d{1,3}(\.\d{1,3}){3}$') { throw 'Use the Mac Tailscale IPv4 address.' }
$tools = Join-Path $Root 'tools'
New-Item -ItemType Directory -Force $Root, $tools, (Join-Path $Root 'jobs'), (Join-Path $Root 'downloads') | Out-Null
& icacls.exe $Root /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot protect build directory ACLs' }
Start-Transcript -Path (Join-Path $Root 'setup.log') -Append
try {
    foreach ($name in @('build_common.py', 'build_gateway.py', 'build_runner.py', 'Build-Windows.ps1', 'Submit-Build.ps1')) {
        Copy-Item (Join-Path $PSScriptRoot $name) (Join-Path $tools $name) -Force
    }
    $pythonRoot = Join-Path $Root 'python'
    $python = Join-Path $pythonRoot 'python.exe'
    if (!(Test-Path $python)) {
        $zip = Join-Path $Root 'downloads\python-3.13.12-embed-amd64.zip'
        Invoke-WebRequest -UseBasicParsing 'https://www.python.org/ftp/python/3.13.12/python-3.13.12-embed-amd64.zip' -OutFile $zip
        if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant() -ne '76f238f606250c87c6beac75dccd35ee99070a13490555936abb6cb64ecce3d0') { throw 'Python ZIP hash differs' }
        Expand-Archive $zip $pythonRoot -Force
        $signature = Get-AuthenticodeSignature $python
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Python Software Foundation') { throw 'Python signature is not trusted' }
    }
    # Embeddable Python otherwise omits the script directory from sys.path.
    @('python313.zip', '.', '..\tools') | Set-Content (Join-Path $pythonRoot 'python313._pth') -Encoding Ascii
    $authorizedKeys = Join-Path $env:ProgramData 'ssh\administrators_authorized_keys'
    if (!(Test-Path (Join-Path $env:ProgramData 'ssh\sshd_config'))) { throw 'Configure OpenSSH Server first.' }
    $command = (($python -replace '\\', '/') + ' ' + ((Join-Path $tools 'build_gateway.py') -replace '\\', '/') + ' --root ' + ($Root -replace '\\', '/'))
    $line = 'restrict,from="' + $ClientAddress + '",command="' + $command + '" ' + $ClientPublicKey
    $existing = if (Test-Path $authorizedKeys) { [IO.File]::ReadAllText($authorizedKeys) } else { '' }
    $keyBody = ($ClientPublicKey -split ' ')[1]
    if ($existing -match [regex]::Escape($keyBody)) {
        if (!$existing.Contains($line)) { throw 'This build key already has different options. Review it manually; it was not changed.' }
    } else {
        if ($existing) { Copy-Item $authorizedKeys (Join-Path $Root ('authorized-keys-backup-' + (Get-Date -Format 'yyyyMMddHHmmss') + '.txt')) }
        [IO.File]::AppendAllText($authorizedKeys, "`r`n$line`r`n", (New-Object Text.UTF8Encoding($false)))
    }
    & icacls.exe $authorizedKeys /inheritance:r /grant:r '*S-1-5-18:F' '*S-1-5-32-544:F' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot protect build key ACLs' }
    # Keep the existing Tailscale-only listener, firewall and publication key.
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $install = if (Test-Path $vswhere) { & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath } else { $null }
    if (!$install) {
        $installer = Join-Path $Root 'downloads\vs_BuildTools.exe'
        Invoke-WebRequest -UseBasicParsing 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile $installer
        $signature = Get-AuthenticodeSignature $installer
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') { throw 'Visual Studio installer signature is not trusted' }
        $arguments = @('--quiet', '--wait', '--norestart', '--nocache', '--installPath', 'C:\CViewBuildTools',
                       '--add', 'Microsoft.VisualStudio.Workload.VCTools', '--includeRecommended',
                       '--add', 'Microsoft.VisualStudio.Component.VC.Tools.ARM64')
        $process = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru
        if ($process.ExitCode -notin @(0, 3010)) { throw "Visual Studio installation failed: $($process.ExitCode)" }
        if ($process.ExitCode -eq 3010) { Write-Warning 'Visual Studio requests a restart. Finish setup, then restart before building if preflight fails.' }
        $install = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath
        if (!$install) { throw 'Visual Studio installation did not provide both architectures.' }
    }
    & (Join-Path $install 'Common7\Tools\Launch-VsDevShell.ps1') -Arch amd64 -HostArch amd64 -SkipAutomaticLocation
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    if (!(Get-Command rustup -ErrorAction SilentlyContinue)) {
        $rustup = Join-Path $Root 'downloads\rustup-init.exe'
        $url = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
        Invoke-WebRequest -UseBasicParsing $url -OutFile $rustup
        $hashResponse = (Invoke-WebRequest -UseBasicParsing ($url + '.sha256')).Content
        $hashText = if ($hashResponse -is [byte[]]) { [Text.Encoding]::UTF8.GetString($hashResponse) } else { [string]$hashResponse }
        $expected = ($hashText.Trim() -split '\s+')[0]
        if ($expected -notmatch '^[a-fA-F0-9]{64}$') { throw 'Invalid rustup checksum response' }
        if ((Get-FileHash $rustup -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected.ToLowerInvariant()) { throw 'rustup SHA-256 differs' }
        & $rustup -y --profile minimal --default-toolchain stable-x86_64-pc-windows-msvc --no-modify-path
        if ($LASTEXITCODE -ne 0) { throw 'rustup installation failed' }
    }
    foreach ($arguments in @(@('toolchain', 'install', 'stable', '--profile', 'minimal'),
                             @('component', 'add', '--toolchain', 'stable', 'clippy', 'rustfmt'),
                             @('target', 'add', '--toolchain', 'stable', 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc'))) {
        & rustup @arguments
        if ($LASTEXITCODE -ne 0) { throw 'Rust toolchain setup failed' }
    }
    $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
    & cargo install tauri-cli --version '2.12.3' --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tauri CLI installation failed' }
    & (Join-Path $tools 'Build-Windows.ps1') -Root $Root -CheckOnly
    if ($LASTEXITCODE -ne 0) { throw 'Windows preflight failed' }
    Write-Output 'CView Windows build environment is ready. Connect the Mac worker, then submit a build.'
} finally { Stop-Transcript }

#Requires -Version 5.1
#Requires -RunAsAdministrator
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)][string]$PublicKeyPath,
    [Parameter(Mandatory = $true)][string]$PythonZip,
    [Parameter(Mandatory = $true)][ValidatePattern('^100\.[0-9]+\.[0-9]+\.[0-9]+$')][string]$ClientAddress
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = 'C:\CViewUpdates'
$tools = Join-Path $root 'tools'
$runtime = Join-Path $tools 'python'
$keyPath = 'C:\ProgramData\ssh\administrators_authorized_keys'
$publicKey = [IO.File]::ReadAllText((Resolve-Path $PublicKeyPath)).Trim()
if ($publicKey -notmatch '^ssh-ed25519 [A-Za-z0-9+/]+={0,2}( [^\r\n]*)?$') { throw 'An Ed25519 public key is required.' }
$keyParts = $publicKey -split ' '
$keyBody = $keyParts[0] + ' ' + $keyParts[1]
# Official Python.org 3.13.12 embeddable amd64 package. No PATH or system installation changes.
$expectedZip = '76f238f606250c87c6beac75dccd35ee99070a13490555936abb6cb64ecce3d0'
if ((Get-FileHash -LiteralPath $PythonZip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedZip) { throw 'Python package SHA-256 mismatch.' }
foreach ($name in @('release_gateway.py', 'publish.py', 'check_server.py', 'Publish-UploadedRelease.ps1')) {
    if (-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot $name))) { throw "Missing source: $name" }
}
if (-not $PSCmdlet.ShouldProcess("$ClientAddress -> CView release protocol", 'Install restricted SSH release key and publishing runtime')) { return }
$backup = Join-Path $tools ('automation-before-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $backup | Out-Null
if (Test-Path $keyPath) { Copy-Item $keyPath (Join-Path $backup 'administrators_authorized_keys') }
foreach ($name in @('release_gateway.py', 'publish.py', 'check_server.py', 'Publish-UploadedRelease.ps1')) {
    $destination = Join-Path $tools $name
    if (Test-Path $destination) { Copy-Item $destination (Join-Path $backup $name) }
    Copy-Item (Join-Path $PSScriptRoot $name) $destination -Force
}
if (-not (Test-Path $runtime)) {
    Expand-Archive -LiteralPath $PythonZip -DestinationPath $runtime
} elseif (-not (Test-Path (Join-Path $runtime 'python313.dll'))) {
    throw 'Unexpected existing Python runtime; inspect it first.'
}
$python = Join-Path $runtime 'python.exe'
$signature = Get-AuthenticodeSignature $python
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*Python Software Foundation*') { throw 'Python Authenticode signature verification failed.' }
[IO.File]::WriteAllText((Join-Path $runtime 'python313._pth'), "python313.zip`r`n.`r`n..`r`n", [Text.Encoding]::ASCII)
& $python -I -B -c 'import ssl,sys; print(sys.version)'
if ($LASTEXITCODE -ne 0) { throw 'Python runtime verification failed.' }
$command = 'C:/CViewUpdates/tools/python/python.exe -I -B C:/CViewUpdates/tools/release_gateway.py'
$entry = 'restrict,from="' + $ClientAddress + '",command="' + $command + '" ' + $keyBody + ' cview-release-automation'
$existing = @()
if (Test-Path $keyPath) {
    $existing = @([IO.File]::ReadAllLines($keyPath))
    $sameKey = @($existing | Where-Object { $_ -match [regex]::Escape($keyParts[1]) })
    if ($sameKey.Count -and ($sameKey.Count -ne 1 -or $sameKey[0] -ne $entry)) { throw 'This key already has different permissions; inspect it first.' }
}
if (-not ($existing -contains $entry)) {
    [IO.File]::WriteAllLines($keyPath, @($existing) + @($entry), [Text.Encoding]::ASCII)
}
# Only SYSTEM and Administrators may read/write the administrators key list.
$acl = New-Object System.Security.AccessControl.FileSecurity
$acl.SetOwner((New-Object System.Security.Principal.SecurityIdentifier('S-1-5-18')))
$acl.SetAccessRuleProtection($true, $false)
foreach ($sid in @('S-1-5-18', 'S-1-5-32-544')) {
    $identity = New-Object System.Security.Principal.SecurityIdentifier($sid)
    $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($identity, 'FullControl', 'Allow')
    $acl.AddAccessRule($rule)
}
Set-Acl -LiteralPath $keyPath -AclObject $acl
& "$env:WINDIR\System32\OpenSSH\sshd.exe" -t
if ($LASTEXITCODE -ne 0) { throw 'SSH configuration verification failed.' }
Write-Host "Installed. The key allows release operations only from $ClientAddress; ordinary administrator password login is unchanged."
Write-Host "Backup: $backup"

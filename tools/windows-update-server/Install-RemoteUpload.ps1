#Requires -Version 5.1
#Requires -RunAsAdministrator
[CmdletBinding(SupportsShouldProcess = $true)]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = 'C:\CViewUpdates'
$incoming = Join-Path $root 'incoming'
$shareName = 'CViewUpload$'
$publisher = Join-Path $root 'tools\publish.py'
$helper = Join-Path $PSScriptRoot 'Publish-UploadedRelease.ps1'
if (-not (Test-Path $publisher -PathType Leaf)) { throw 'Existing CView publisher is missing.' }
if (-not (Test-Path $helper -PathType Leaf)) { throw 'Upload publishing helper is missing.' }
$account = Get-LocalUser | Where-Object { $_.SID.Value -match '-500$' }
if (-not $account -or -not $account.Enabled) { throw 'The existing local administrator account is unavailable.' }
$identity = "$env:COMPUTERNAME\$($account.Name)"
$existing = Get-SmbShare -Name $shareName -ErrorAction SilentlyContinue
if ($existing -and $existing.Path -ne $incoming) { throw 'Share name is already used by another path.' }
if ($existing) {
    $access = @(Get-SmbShareAccess -Name $shareName)
    if ($access.Count -ne 1 -or $access[0].AccountName -ne $identity -or
        $access[0].AccessRight -ne 'Full' -or $access[0].AccessControlType -ne 'Allow') {
        throw 'Existing share permissions differ; inspect them before changing anything.'
    }
}
if (-not $existing -and (Test-Path $incoming) -and @(Get-ChildItem $incoming -Force).Count -gt 0) {
    throw 'The incoming directory already contains files; inspect it before installation.'
}
if ($PSCmdlet.ShouldProcess("$incoming as \\$env:COMPUTERNAME\$shareName", "Create encrypted SMB upload share for $identity only")) {
    New-Item -ItemType Directory -Path $incoming -Force | Out-Null
    & icacls.exe $incoming '/inheritance:r' '/grant:r' '*S-1-5-18:(OI)(CI)(F)' '*S-1-5-32-544:(OI)(CI)(F)' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Cannot secure the incoming directory.' }
    Copy-Item $helper (Join-Path $root 'tools\Publish-UploadedRelease.ps1') -Force
    if (-not $existing) {
        New-SmbShare -Name $shareName -Path $incoming -FullAccess $identity -EncryptData $true -FolderEnumerationMode AccessBased -CachingMode None -Description 'CView release upload staging; use the existing administrator account over Tailscale.' | Out-Null
    } else {
        Set-SmbShare -Name $shareName -EncryptData $true -FolderEnumerationMode AccessBased -CachingMode None -Force | Out-Null
    }
    Get-SmbShare -Name $shareName | Select-Object Name, Path, EncryptData, FolderEnumerationMode, CachingMode
    Get-SmbShareAccess -Name $shareName
    Write-Host 'Upload: \\100.117.247.29\CViewUpload$\<version>'
    Write-Host 'Publish on the server: powershell -NoProfile -File C:\CViewUpdates\tools\Publish-UploadedRelease.ps1 -Version <version>'
}

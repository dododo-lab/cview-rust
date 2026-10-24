#Requires -Version 5.1
#Requires -RunAsAdministrator
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?$')]
    [string]$HostName,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Fa-f0-9 ]+$')]
    [string]$CertificateThumbprint,
    [ValidatePattern('^[A-Za-z0-9_-]+$')]
    [string]$SiteName = 'CViewUpdates',
    [string]$RootPath = 'C:\CViewUpdates'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'This installer requires Windows Server.' }
$RootPath = [IO.Path]::GetFullPath($RootPath)
if ($RootPath.TrimEnd('\') -eq [IO.Path]::GetPathRoot($RootPath).TrimEnd('\')) {
    throw 'RootPath must be a dedicated directory, not a drive root.'
}
$thumbprint = $CertificateThumbprint.Replace(' ', '').ToUpperInvariant()
$certificate = Get-Item "Cert:\LocalMachine\My\$thumbprint"
if (-not $certificate.HasPrivateKey -or $certificate.NotAfter -le (Get-Date)) {
    throw 'The HTTPS certificate must have a private key and must not be expired.'
}
$markerPath = Join-Path $RootPath 'server.json'
$publicPath = Join-Path $RootPath 'site'
$managed = Test-Path $markerPath
if ($managed) {
    $existing = Get-Content $markerPath -Raw | ConvertFrom-Json
    if ($existing.siteName -ne $SiteName -or $existing.hostName -ne $HostName) {
        throw 'This directory belongs to a different site. Choose a new RootPath.'
    }
} elseif ((Test-Path $RootPath) -and @(Get-ChildItem $RootPath -Force).Count -gt 0) {
    throw 'RootPath is not empty and is not managed by this installer.'
}
$features = Install-WindowsFeature -Name Web-Server, Web-Static-Content, Web-Http-Logging -IncludeManagementTools
if (-not $features.Success) { throw 'IIS feature installation failed.' }
if ($features.RestartNeeded -eq 'Yes') { throw 'Restart Windows Server and rerun this installer.' }
Import-Module WebAdministration
$existingSite = Get-Website | Where-Object { $_.Name -eq $SiteName }
if ($existingSite -and -not $managed) { throw 'An unmanaged IIS site already has this name.' }
$pool = "$SiteName-Pool"
if ((Test-Path "IIS:\AppPools\$pool") -and -not $managed) {
    throw 'An unmanaged application pool already has this name.'
}
if ($existingSite -and [IO.Path]::GetFullPath($existingSite.physicalPath) -ne $publicPath) {
    throw 'An existing IIS site has this name but a different directory.'
}
# Do not take over another site's HTTPS hostname binding.
foreach ($site in Get-Website) {
    if ($site.Name -eq $SiteName) { continue }
    foreach ($binding in $site.bindings.Collection) {
        if ($binding.protocol -ne 'https') { continue }
        $parts = $binding.bindingInformation -split ':'
        if ($parts[-2] -eq '443' -and $parts[-1] -eq $HostName) {
            throw "Another IIS site has a conflicting HTTPS binding: $($site.Name)"
        }
    }
}
New-Item -ItemType Directory -Path $publicPath -Force | Out-Null
# Only administrators and SYSTEM may publish; IIS has read access to public files.
& icacls.exe $RootPath '/inheritance:r' '/grant:r' '*S-1-5-18:(OI)(CI)(F)' '*S-1-5-32-544:(OI)(CI)(F)' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot secure the update root directory.' }
& icacls.exe $publicPath '/grant:r' '*S-1-5-32-568:(OI)(CI)(RX)' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot grant IIS read access.' }
foreach ($file in @('web.config', 'index.html', 'style.css', 'health.json')) {
    Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $publicPath $file) -Force
}
@{ siteName = $SiteName; hostName = $HostName; rootPath = $RootPath; endpoint = "https://$HostName/releases/latest.json" } |
    ConvertTo-Json | Set-Content $markerPath -Encoding UTF8
if (-not (Test-Path "IIS:\AppPools\$pool")) { New-WebAppPool -Name $pool | Out-Null }
Set-ItemProperty "IIS:\AppPools\$pool" -Name managedRuntimeVersion -Value ''
if (-not $existingSite) {
    New-Website -Name $SiteName -PhysicalPath $publicPath -ApplicationPool $pool -Port 443 -HostHeader $HostName -Ssl | Out-Null
}
Set-WebBinding -Name $SiteName -Protocol https -Port 443 -HostHeader $HostName -PropertyName sslFlags -Value 1
$binding = Get-WebBinding -Name $SiteName -Protocol https -Port 443 -HostHeader $HostName
if (-not $binding) { throw 'Expected HTTPS binding was not found.' }
$binding.AddSslCertificate($thumbprint, 'My')
$rule = "CViewUpdates-$SiteName-HTTPS"
if (-not (Get-NetFirewallRule -Name $rule -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -Name $rule -DisplayName "CView updates ($SiteName) HTTPS" -Direction Inbound -Action Allow -Protocol TCP -LocalPort 443 | Out-Null
}
Start-Website -Name $SiteName
Write-Host "Configured IIS site: $SiteName"
Write-Host "Public directory: $publicPath"
Write-Host "Feed: https://$HostName/releases/latest.json"
Write-Host 'Verify DNS/TLS and health.json from a client. Publish the first release before testing the feed.'

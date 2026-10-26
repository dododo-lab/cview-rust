#Requires -Version 5.1
#Requires -RunAsAdministrator
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$')]
    [string]$Version,
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = 'C:\CViewUpdates'
$baseUrl = 'https://cv.dododo.app'
$folder = Join-Path (Join-Path $root 'incoming') $Version
$artifacts = @(
    (Join-Path $folder "CView_${Version}_arm64.dmg"),
    (Join-Path $folder "CView_${Version}_x64-setup.exe")
)
$notes = Join-Path $folder 'notes.txt'
foreach ($path in @($folder) + $artifacts + @($notes)) {
    $item = Get-Item -LiteralPath $path
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Links are not allowed: $path" }
    if ($path -ne $folder -and ($item.PSIsContainer -or $item.Length -eq 0)) { throw "A nonempty file is required: $path" }
}
$latest = Join-Path $root 'site\releases\latest.json'
if (Test-Path $latest) {
    $current = (Get-Content $latest -Raw | ConvertFrom-Json).tag_name -replace '^[vV]', ''
    if ([version]$Version -le [version]$current) { throw "Version must be newer than $current." }
}
if (Test-Path (Join-Path $root "site\releases\$Version")) { throw 'This version already exists.' }
Get-FileHash -LiteralPath $artifacts -Algorithm SHA256 | Format-Table -AutoSize
if ($ValidateOnly) {
    Write-Host 'Upload files and version validated. Nothing was published. Signing/notarization must be checked during the build.'
    return
}
if ($PSCmdlet.ShouldProcess("$baseUrl/releases/latest.json", "Publish CView $Version with both platform installers")) {
    $python = Join-Path $root 'tools\python\python.exe'
    if (-not (Test-Path $python)) { throw 'Run Install-ReleaseAutomation.ps1 to install the publishing runtime.' }
    & $python -I -B (Join-Path $root 'tools\publish.py') --root (Join-Path $root 'site') --base-url $baseUrl --version $Version --artifact $artifacts[0] --artifact $artifacts[1] --notes-file $notes
    if ($LASTEXITCODE -ne 0) { throw 'Publishing failed. Inspect the publisher output; do not report a completed release.' }
    & $python -I -B (Join-Path $root 'tools\check_server.py') --base-url $baseUrl --download
    if ($LASTEXITCODE -ne 0) { throw 'Release was published, but download verification failed. Investigate before reporting completion.' }
    Write-Host "Published and verified CView $Version. Upload files remain in $folder."
}

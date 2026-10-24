#Requires -Version 5.1
#Requires -RunAsAdministrator
[CmdletBinding()]
param([string]$OutputPath = 'C:\CViewUpdates\incoming\_powershell-ssh-verification.json')
$ErrorActionPreference = 'Stop'
$f = Get-NetFirewallRule -Name 'CView-Tailscale-PowerShell-SSH'
$s = Get-Service sshd
$result = @{
    server = $env:COMPUTERNAME
    status = [string]$s.Status
    startType = [string]$s.StartType
    defaultShell = (Get-ItemProperty HKLM:\SOFTWARE\OpenSSH).DefaultShell
    startup = (Get-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Services\sshd | Select-Object Start,DelayedAutostart,DependOnService)
    listen = @(Get-NetTCPConnection -State Listen -LocalPort 22 | Select-Object LocalAddress,LocalPort,OwningProcess)
    rule = @{ enabled = [string]$f.Enabled; action = [string]$f.Action; direction = [string]$f.Direction }
    address = ($f | Get-NetFirewallAddressFilter | Select-Object LocalAddress,RemoteAddress)
    interfaces = ($f | Get-NetFirewallInterfaceFilter | Select-Object InterfaceAlias)
    oldRuleEnabled = [string](Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP').Enabled
    publicKey = (Get-Content C:\ProgramData\ssh\ssh_host_ed25519_key.pub -Raw).Trim()
    fingerprint = (& C:\Windows\System32\OpenSSH\ssh-keygen.exe -lf C:\ProgramData\ssh\ssh_host_ed25519_key.pub)
    hostKeyAcls = @(Get-ChildItem C:\ProgramData\ssh\ssh_host_*_key | Get-Acl | Select-Object Path,Owner,Sddl)
    config = [IO.File]::ReadAllText('C:\ProgramData\ssh\sshd_config')
}
if ($s.Status -ne 'Running') { throw 'SSH service is not running.' }
if (@($result.listen | Where-Object LocalAddress -ne '100.117.247.29').Count) { throw 'Unexpected SSH listener.' }
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $OutputPath -Encoding UTF8
Write-Host "Saved nonsecret SSH verification: $OutputPath"

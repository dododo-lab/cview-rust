#Requires -Version 5.1
#Requires -RunAsAdministrator
[CmdletBinding(SupportsShouldProcess = $true)]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$address = '100.117.247.29'
$sshRoot = Join-Path $env:ProgramData 'ssh'
$configPath = Join-Path $sshRoot 'sshd_config'
$sshd = Join-Path $env:WINDIR 'System32\OpenSSH\sshd.exe'
$keygen = Join-Path $env:WINDIR 'System32\OpenSSH\ssh-keygen.exe'
$shell = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$registry = 'HKLM:\SOFTWARE\OpenSSH'
$ruleName = 'CView-Tailscale-PowerShell-SSH'
$marker = '# Managed by CView Install-TailscalePowerShell.ps1'
$backup = 'C:\CViewUpdates\tools\remote-powershell-before-20261024'
$service = Get-Service sshd
$tailscale = Get-Service Tailscale
$interface = Get-NetIPAddress -IPAddress $address -AddressFamily IPv4
$account = Get-LocalUser | Where-Object { $_.SID.Value -match '-500$' }
if (-not $account -or -not $account.Enabled) { throw 'The existing administrator account is unavailable.' }
if (-not (Test-Path $sshd) -or -not (Test-Path $shell)) { throw 'Built-in OpenSSH Server and Windows PowerShell are required.' }
if ($tailscale.Status -ne 'Running') { throw 'Tailscale must be running first.' }
$managed = (Test-Path $configPath) -and (Get-Content $configPath -Raw).StartsWith($marker)
if ((Test-Path $configPath) -and -not $managed) { throw 'Existing SSH configuration is not managed by this installer; inspect it first.' }
if ($service.Status -eq 'Running' -and -not $managed) { throw 'An existing SSH service is already running; inspect it first.' }
if ((Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue) -and -not $managed) { throw 'Firewall rule name is already in use.' }
$user = $account.Name.ToLowerInvariant()
$config = @"
$marker
Port 22
AddressFamily inet
ListenAddress $address
AllowUsers $user
PasswordAuthentication yes
PubkeyAuthentication yes
PermitEmptyPasswords no
MaxAuthTries 3
LoginGraceTime 30
LogLevel VERBOSE
AllowAgentForwarding no
AllowTcpForwarding no
GatewayPorts no
AuthorizedKeysFile .ssh/authorized_keys
Subsystem sftp sftp-server.exe
Match Group administrators
    AuthorizedKeysFile __PROGRAMDATA__/ssh/administrators_authorized_keys
"@
if (-not $PSCmdlet.ShouldProcess("$address`:22 for $user over $($interface.InterfaceAlias)", 'Enable SSH with Windows PowerShell, Tailscale-only listener/firewall and automatic service startup')) { return }
if (-not (Test-Path $backup)) {
    New-Item -ItemType Directory -Path $backup | Out-Null
    $oldShell = Get-ItemProperty $registry -Name DefaultShell -ErrorAction SilentlyContinue
    $oldRule = Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue
    @{
        serviceStartType = [string]$service.StartType
        serviceStatus = [string]$service.Status
        dependencies = @($service.ServicesDependedOn | ForEach-Object Name)
        defaultShell = $(if ($oldShell) { $oldShell.DefaultShell } else { $null })
        defaultFirewallEnabled = $(if ($oldRule) { [string]$oldRule.Enabled } else { $null })
        configExisted = (Test-Path $configPath)
    } | ConvertTo-Json | Set-Content (Join-Path $backup 'state.json') -Encoding UTF8
    if (Test-Path $configPath) { Copy-Item $configPath (Join-Path $backup 'sshd_config') }
}
New-Item -ItemType Directory -Path $sshRoot -Force | Out-Null
$candidate = Join-Path $sshRoot 'sshd_config.cview-candidate'
[IO.File]::WriteAllText($candidate, $config + "`r`n", [Text.Encoding]::ASCII)
& $keygen -A
if ($LASTEXITCODE -ne 0) { throw 'SSH host key initialization failed.' }
# ssh-keygen run interactively can leave the creating user's SID on private
# host keys. The LocalSystem service requires SYSTEM-owned server key files.
foreach ($name in @('ssh_host_rsa_key', 'ssh_host_ecdsa_key', 'ssh_host_ed25519_key')) {
    $hostKey = Join-Path $sshRoot $name
    & icacls.exe $hostKey '/setowner' '*S-1-5-18' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Cannot set SSH host key owner: $name" }
    & icacls.exe $hostKey '/inheritance:r' '/grant:r' '*S-1-5-18:(F)' '*S-1-5-32-544:(F)' '/remove:g' ('*' + $account.SID.Value) | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Cannot secure the SSH host key: $name" }
}
& $sshd -t -f $candidate
if ($LASTEXITCODE -ne 0) { throw 'SSH configuration validation failed; active configuration was not replaced.' }
Copy-Item $candidate $configPath -Force
if (-not (Test-Path $registry)) { New-Item -Path $registry | Out-Null }
New-ItemProperty -Path $registry -Name DefaultShell -Value $shell -PropertyType String -Force | Out-Null
if (Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue) {
    Disable-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' | Out-Null
}
$rule = @{
    Name = $ruleName; DisplayName = 'CView PowerShell SSH - Tailscale only'
    Enabled = 'True'; Direction = 'Inbound'; Action = 'Allow'; Protocol = 'TCP'
    LocalPort = 22; LocalAddress = $address; RemoteAddress = '100.64.0.0/10'
    InterfaceAlias = $interface.InterfaceAlias; Profile = 'Any'; Program = $sshd
}
if (Get-NetFirewallRule -Name $ruleName -ErrorAction SilentlyContinue) {
    $rule.Remove('DisplayName')
    Set-NetFirewallRule @rule | Out-Null
} else {
    New-NetFirewallRule @rule | Out-Null
}
$dependencies = @($service.ServicesDependedOn | ForEach-Object Name) + @('Tailscale') | Select-Object -Unique
& sc.exe config sshd start= delayed-auto depend= ($dependencies -join '/') | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot configure SSH service startup.' }
& sc.exe failure sshd reset= 86400 actions= restart/10000/restart/30000/restart/60000 | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot configure SSH service recovery.' }
if ($service.Status -eq 'Running') { Restart-Service sshd } else { Start-Service sshd }
$service = Get-Service sshd
$service.WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
$listeners = @(Get-NetTCPConnection -State Listen -LocalPort 22)
if (-not $listeners.Count -or @($listeners | Where-Object LocalAddress -ne $address).Count) { throw 'SSH listener verification failed.' }
Write-Host "Ready: ssh $user@$address"
Write-Host "Default shell: $shell"
& $keygen -lf (Join-Path $sshRoot 'ssh_host_ed25519_key.pub')
Get-NetTCPConnection -State Listen -LocalPort 22 | Format-Table LocalAddress,LocalPort
Get-Service sshd | Format-Table Name,Status,StartType

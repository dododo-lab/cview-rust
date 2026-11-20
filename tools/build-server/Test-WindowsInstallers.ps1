# Run both current-user NSIS installers in an isolated Windows profile.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Directory,
    [Parameter(Mandatory)][ValidateSet('Prepare', 'Launch', 'Worker', 'Cleanup')][string]$Action
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Directory = (Resolve-Path -LiteralPath $Directory).Path
$report = Join-Path $Directory 'installer-smoke.json'
$credentialFile = Join-Path $Directory 'test-account.json'
$testArchive = Join-Path $Directory 'x64-tests.zip'
$installers = @(
    @{ Architecture = 'x64'; Machine = 0x8664 },
    @{ Architecture = 'arm64'; Machine = 0xaa64 }
)

if ($Action -eq 'Worker') {
    $results = @()
    $workerError = $null
    $testResult = $null
    if (Test-Path -LiteralPath $testArchive) {
        try {
            $suiteDir = Join-Path $Directory 'x64-tests'
            Expand-Archive -LiteralPath $testArchive -DestinationPath $suiteDir
            $suites = @(Get-ChildItem -LiteralPath $suiteDir -File -Filter '*.exe' | Sort-Object Name)
            if ($suites.Count -eq 0) { throw 'No x64 runtime test suites' }
            $passed = 0
            $ignored = 0
            foreach ($suite in $suites) {
                $log = Join-Path $suiteDir ($suite.BaseName + '.log')
                & $suite.FullName --test-threads 2 > $log
                $exitCode = $LASTEXITCODE
                $output = Get-Content -LiteralPath $log -Raw
                if ($exitCode -ne 0 -or $output -notmatch '(?m)^test result: ok\. (\d+) passed; 0 failed; (\d+) ignored') {
                    throw "Windows test suite $($suite.Name) failed (exit $exitCode): $output"
                }
                $passed += [int]$Matches[1]
                $ignored += [int]$Matches[2]
            }
            $testResult = [pscustomobject]@{
                sha256 = (Get-FileHash -LiteralPath $testArchive -Algorithm SHA256).Hash.ToLowerInvariant()
                suites = $suites.Count
                passed = $passed
                failed = 0
                ignored = $ignored
            }
        } catch {
            $workerError = $_.Exception.Message
        }
    }
    foreach ($entry in $installers) {
        if ($workerError) { break }
        $app = $null
        $arch = $entry.Architecture
        $installer = Get-ChildItem -LiteralPath $Directory -Filter "*_${arch}-setup.exe" -File
        $destination = Join-Path $env:LOCALAPPDATA "CViewSmoke-$arch"
        try {
            if (@($installer).Count -ne 1) { throw "Expected exactly one $arch installer" }
            $setup = Start-Process -FilePath $installer.FullName -ArgumentList "/S /D=$destination" -Wait -PassThru
            if ($setup.ExitCode -ne 0) { throw "$arch installer exited with $($setup.ExitCode)" }
            $exe = Join-Path $destination 'cview-app.exe'
            $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($exe))
            try {
                if ($reader.ReadUInt16() -ne 0x5a4d) { throw 'Missing MZ header' }
                $reader.BaseStream.Position = 0x3c
                $reader.BaseStream.Position = $reader.ReadUInt32()
                if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne $entry.Machine) {
                    throw "$arch installed binary has the wrong PE architecture"
                }
            } finally { $reader.Dispose() }
            $app = Start-Process -FilePath $exe -WorkingDirectory $destination -PassThru
            $deadline = (Get-Date).AddSeconds(75)
            $webview = $false
            do {
                Start-Sleep -Seconds 2
                $app.Refresh()
                if ($app.HasExited) { throw "$arch app exited with $($app.ExitCode)" }
                $webview = [bool](Get-CimInstance Win32_Process -Filter "ParentProcessId=$($app.Id) AND Name='msedgewebview2.exe'")
            } until ($webview -or (Get-Date) -ge $deadline)
            if (!$webview) { throw "$arch app did not start its WebView2 process" }
            $results += [pscustomobject]@{
                architecture = $arch
                installer_sha256 = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
                installed = $true
                webview2_started = $true
            }
        } catch {
            $workerError = $_.Exception.Message
            break
        } finally {
            if ($app -and !$app.HasExited) { Stop-Process -InputObject $app -Force }
            $uninstaller = Join-Path $destination 'uninstall.exe'
            if (Test-Path -LiteralPath $uninstaller) {
                Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait
            }
        }
    }
    [pscustomobject]@{
        host_architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        session_id = (Get-Process -Id $PID).SessionId
        installers = $results
        x64_tests = $testResult
        error = $workerError
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $report -Encoding UTF8
    if ($workerError) { exit 1 }
    exit 0
}

if ($Action -eq 'Prepare') {
    if ((Test-Path -LiteralPath $report) -or (Test-Path -LiteralPath $credentialFile)) {
        throw "Use a fresh smoke-test directory"
    }
    $desktopUser = (Get-CimInstance Win32_ComputerSystem).UserName
    if (!$desktopUser) { throw 'No user is logged into the Windows desktop' }
    $userName = 'CViewQa' + [Guid]::NewGuid().ToString('N').Substring(0, 10)
    $key = [Guid]::NewGuid().ToString('N') + 'aA1!'
    $secure = ConvertTo-SecureString $key -AsPlainText -Force
    $user = New-LocalUser -Name $userName -Password $secure -Description 'Temporary CView installer test'
    try {
        $groups = Get-LocalGroupMember -SID 'S-1-5-32-545'
        if (!$groups.Where({ $_.SID -eq $user.SID })) {
            Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $userName
        }
        & icacls.exe $Directory /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)(F)' '*S-1-5-32-544:(OI)(CI)(F)' `
            "${desktopUser}:(OI)(CI)(RX)" "*$($user.SID):(OI)(CI)(M)" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Failed to restrict access to the test directory' }
        Add-Type -AssemblyName System.Security
        $protected = [Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($key),
            $null, [Security.Cryptography.DataProtectionScope]::LocalMachine)
        [pscustomobject]@{ Name = $userName; SID = [string]$user.SID; Key = [Convert]::ToBase64String($protected) } |
            ConvertTo-Json | Set-Content -LiteralPath $credentialFile -Encoding ASCII
    } catch {
        Remove-LocalUser -SID $user.SID
        throw
    }
    exit 0
}

$saved = Get-Content -LiteralPath $credentialFile -Raw | ConvertFrom-Json
if ($Action -eq 'Cleanup') {
    $user = Get-LocalUser -SID $saved.SID
    if ($saved.Name -notmatch '^CViewQa[0-9a-f]{10}$' -or $user.Name -ne $saved.Name -or
        $user.Description -ne 'Temporary CView installer test') { throw 'Unexpected test account' }
    Remove-LocalUser -SID $user.SID
    Get-CimInstance Win32_UserProfile -Filter "SID='$($user.SID)'" |
        Where-Object { !$_.Loaded } | Remove-CimInstance
    & icacls.exe $Directory /remove:g "*$($user.SID)" | Out-Null
    Remove-Item -LiteralPath $credentialFile
    return
}

Add-Type -AssemblyName System.Security
$bytes = [Security.Cryptography.ProtectedData]::Unprotect([Convert]::FromBase64String($saved.Key),
    $null, [Security.Cryptography.DataProtectionScope]::LocalMachine)
$password = ConvertTo-SecureString ([Text.Encoding]::UTF8.GetString($bytes)) -AsPlainText -Force
$credential = [pscredential]::new("$env:COMPUTERNAME\$($saved.Name)", $password)
$process = Start-Process -FilePath "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" `
    -ArgumentList "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Directory `"$Directory`" -Action Worker" `
    -Credential $credential -LoadUserProfile -WorkingDirectory $Directory -PassThru
if (!$process.WaitForExit(480000)) {
    $process.Kill()
    throw 'The isolated Windows verification did not exit within eight minutes'
}
if (!(Test-Path -LiteralPath $report)) { throw "Missing test report (process exit: $($process.ExitCode))" }
Get-Content -LiteralPath $report -Raw
exit $process.ExitCode

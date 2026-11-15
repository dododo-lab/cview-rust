# Upload/edit a complete source tree on the server, then call this script.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$SourcePath,
    [string]$Root = 'C:\CViewBuild',
    [switch]$Wait
)
$ErrorActionPreference = 'Stop'
$python = Join-Path $Root 'python\python.exe'
$tools = Join-Path $Root 'tools'
$SourcePath = (Resolve-Path $SourcePath).Path
$code = @'
import json, pathlib, sys, tempfile, time
import build_common as c
root, source, wait = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3] == 'True'
with tempfile.TemporaryDirectory(dir=root) as tmp:
    archive = pathlib.Path(tmp) / 'source.zip'
    c.snapshot(source, archive)
    job = c.job_id()
    with archive.open('rb') as stream:
        result = c.submit(root, job, stream, c.describe(archive))
    print(json.dumps(result, indent=2), flush=True)
    print('Artifacts: ' + str(root / 'jobs' / job / 'artifacts'), flush=True)
    if wait:
        print('Waiting for Windows and the Mac worker...', flush=True)
        while True:
            result = c.status(root, job)
            if result['state'] != 'pending':
                print(json.dumps(result, indent=2), flush=True)
                sys.exit(0 if result['state'] == 'succeeded' else 1)
            time.sleep(5)
'@
& $python -c $code $Root $SourcePath ([bool]$Wait).ToString()
if ($LASTEXITCODE -ne 0) { throw "Build submission or build failed (exit $LASTEXITCODE)." }

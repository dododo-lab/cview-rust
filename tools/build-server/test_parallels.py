#!/usr/bin/env python3
"""Verify a matched pair of CView installers in a running Parallels guest."""

import argparse
import base64
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import threading
import time
import uuid
import zipfile

import build_common as common

TRANSPORT_ERRORS = ('PrlJob_GetRetCode: Invalid argument', 'PrlJob_GetResult: Invalid argument')


class TransferHandler(SimpleHTTPRequestHandler):
    def list_directory(self, path):
        self.send_error(404)


def guest(vm, script, desktop=False, retry_transport=False):
    encoded = base64.b64encode(script.encode('utf-16-le')).decode('ascii')
    command = ['prlctl', 'exec', vm]
    if desktop:
        command.append('--current-user')
    command += ['powershell.exe', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                '-EncodedCommand', encoded]
    attempts = 3 if retry_transport else 1
    for attempt in range(attempts):
        result = subprocess.run(command, capture_output=True, text=True, encoding='utf-8',
                                errors='replace', timeout=540)
        if not result.returncode:
            return result.stdout
        error = (result.stdout + result.stderr).strip()
        if attempt == attempts - 1 or not any(message in error for message in TRANSPORT_ERRORS):
            raise RuntimeError(error)
        print('Retrying a Parallels guest command after a transport error', flush=True)
        time.sleep(2)


def available(vm, host):
    addr = ipaddress.IPv4Address(host)
    if not addr.is_private or addr.is_loopback or addr.is_unspecified:
        raise ValueError('Use a private Mac IPv4 address reachable from the Parallels guest')
    with ThreadingHTTPServer((str(addr), 0), TransferHandler) as server:
        output = guest(vm, f"""
$ErrorActionPreference='Stop'
$socket=[Net.Sockets.TcpClient]::new()
try {{
    if (!$socket.ConnectAsync('{addr}', {server.server_port}).Wait(5000) -or !$socket.Connected) {{
        throw 'Windows cannot reach the Mac verification server'
    }}
}} finally {{ $socket.Dispose() }}
[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
(Get-Process -Id $PID).SessionId
""", desktop=True, retry_transport=True)
    if not re.search(r'(?m)^Arm64\s*\n[1-9][0-9]*\s*$', output):
        raise ValueError('The Parallels Windows ARM64 desktop must be running and signed in')


def run(vm, host, installers, x64_tests=None):
    version = re.fullmatch(r'CView_([0-9]+\.[0-9]+\.[0-9]+)_x64-setup\.exe', installers[0].name)
    if not version or installers[1].name != f'CView_{version[1]}_arm64-setup.exe':
        raise ValueError('Expected a matching CView x64 and ARM64 installer pair')
    host = ipaddress.IPv4Address(host)
    if not host.is_private:
        raise ValueError('Use a private IPv4 address reachable from the Parallels guest')
    token = uuid.uuid4().hex
    script = Path(__file__).with_name('Test-WindowsInstallers.ps1')
    if x64_tests is not None and x64_tests.name != common.WINDOWS_TEST_ARCHIVE:
        raise ValueError('Invalid Windows test archive name')
    assets = (*installers, script) if x64_tests is None else (*installers, script, x64_tests)
    hashes = {path.name: common.describe(path)['sha256'] for path in assets}

    with tempfile.TemporaryDirectory(prefix='cview-parallels-') as temporary:
        shared = Path(temporary) / token
        shared.mkdir()
        for asset in assets:
            shutil.copyfile(asset, shared / asset.name)
        handler = partial(TransferHandler, directory=temporary)
        with ThreadingHTTPServer((str(host), 0), handler) as server:
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            base = f'http://{host}:{server.server_port}/{token}'
            setup = f"""$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.Encoding]::UTF8
$directory='C:\\ProgramData\\CView-QA\\{token}'
$script=Join-Path $directory '{script.name}'
"""
            prepared = False
            verified = False
            try:
                # Directory creation and hash-checked downloads can safely be replayed.
                print('Transferring installers and test suites to Windows', flush=True)
                guest(vm, setup + '\nNew-Item -ItemType Directory -Path $directory -Force | Out-Null',
                      retry_transport=True)
                for name, digest in hashes.items():
                    guest(vm, setup + f"""
$path=Join-Path $directory '{name}'
Invoke-WebRequest -Uri '{base}/{name}' -UseBasicParsing -OutFile $path -TimeoutSec 45
if ((Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne '{digest}') {{
    throw 'Transferred file SHA-256 differs: {name}'
}}
""", retry_transport=True)
                print('Preparing the isolated Windows test profile', flush=True)
                guest(vm, setup + '\n& $script -Directory $directory -Action Prepare')
                prepared = True
                print('Running the Windows tests and launching both installed apps', flush=True)
                output = guest(vm, setup + '\n& $script -Directory $directory -Action Launch', desktop=True)
                report, _ = json.JSONDecoder().raw_decode(output[output.index('{'):])
                expected = {arch: {
                    'architecture': arch, 'installer_sha256': hashes[path.name],
                    'installed': True, 'webview2_started': True,
                } for arch, path in zip(('x64', 'arm64'), installers)}
                actual = {item['architecture']: item for item in report['installers']}
                if (report.get('error') or report.get('host_architecture') != 'Arm64'
                        or report.get('session_id', 0) <= 0 or len(report['installers']) != 2
                        or actual != expected):
                    raise ValueError('Windows did not confirm both exact installer hashes and WebView2 startup')
                if x64_tests is not None:
                    report = common.validate_windows_runtime(
                        report, {path.name: {'sha256': hashes[path.name]} for path in installers},
                        {'sha256': hashes[x64_tests.name], 'size': x64_tests.stat().st_size}, version[1])
                    with zipfile.ZipFile(x64_tests) as archive:
                        if report['x64_tests']['suites'] != len(archive.namelist()):
                            raise ValueError('Windows did not execute every archived test suite')
                verified = True
            finally:
                try:
                    if prepared:
                        cleanup = '\n& $script -Directory $directory -Action Cleanup'
                        if verified:
                            cleanup += '\nRemove-Item -LiteralPath $directory -Recurse -Force'
                        guest(vm, setup + cleanup)
                finally:
                    server.shutdown()
                    thread.join()
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--vm', required=True)
    parser.add_argument('--host', required=True, help='Private Mac IPv4 reachable from the guest')
    parser.add_argument('--x64-tests', type=Path)
    parser.add_argument('x64', type=Path)
    parser.add_argument('arm64', type=Path)
    args = parser.parse_args()
    try:
        report = run(args.vm, args.host, (args.x64, args.arm64), args.x64_tests)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        parser.exit(1, f'Windows install verification failed: {error}\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

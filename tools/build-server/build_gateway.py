"""Forced SSH command for the build queue. Never accepts a shell command or path."""
import argparse
import json
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import uuid

import build_common as common


def jobs(root):
    folder = common.safe(root / 'jobs')
    if not folder.exists():
        return []
    return sorted(p.name for p in folder.iterdir() if (common.safe(p) / 'job.json').is_file())


def dispatch(root, request, stream):
    action = request.get('action')
    if action == 'setup-log':
        path = common.safe(root / 'setup.log')
        with path.open('rb') as log:
            log.seek(max(0, path.stat().st_size - 32768))
            return {'text': log.read().decode('utf-8', 'replace')}, None
    if action == 'preflight':
        result = subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                                 '-File', str(Path(__file__).with_name('Build-Windows.ps1')),
                                 '-Root', str(root), '-CheckOnly'], capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise ValueError('Windows toolchain preflight failed: ' + result.stdout + result.stderr)
        return {'protocol': common.PROTOCOL, 'server_name': socket.gethostname(),
                'toolchain': result.stdout.strip(), 'mode': 'build-only',
                'features': [common.WINDOWS_RELEASE_FEATURE]}, None
    if action == 'submit':
        return common.submit(root, request['job'], stream, request['source'],
                             mode=request.get('mode', 'build-only'), build_env=request.get('build_env', {})), None
    if action == 'list':
        return {'jobs': [common.status(root, job) for job in jobs(root)[-20:]]}, None
    if action == 'claim':
        worker = request['worker']
        if not isinstance(worker, str) or not worker.isascii() or not worker.isalnum() or len(worker) != 32:
            raise ValueError('Invalid worker ID')
        with common.locked(root / 'queue.lock', blocking=True):
            for job in jobs(root):
                current = common.status(root, job)
                mac = current['macos']
                # The formal release script already builds/notarizes macOS.
                # Never let an installed Mac worker duplicate that build.
                if current['mode'] != 'build-only':
                    continue
                if request.get('job') and job != common.checked_id(request['job']):
                    continue
                if current['state'] != 'pending':
                    continue
                # Resume an interrupted build on the same registered Mac.
                if mac['state'] == 'queued' or (mac['state'] == 'running' and mac.get('worker') == worker):
                    token = mac.get('token') or uuid.uuid4().hex
                    common.write_json(common.job_path(root, job) / 'macos.json',
                                      {'state': 'running', 'worker': worker, 'token': token, 'started': common.now()})
                    return {'job': common.status(root, job)}, None
        return {'job': None}, None
    job = common.checked_id(request['job'])
    directory = common.job_path(root, job)
    current = common.status(root, job)
    if action == 'status':
        return current, None
    if action in ('source', 'artifact', 'log', 'receipt'):
        if action == 'source':
            path, expected = directory / 'source.zip', current['source']
        elif action == 'artifact':
            name = request['name']
            allowed = {f'CView_{current["version"]}_arm64.dmg',
                       *(f'CView_{current["version"]}_{arch}-setup.exe' for arch in common.TARGETS)}
            if name not in allowed:
                raise ValueError('Invalid artifact name')
            platform = 'macos' if name.endswith('.dmg') else 'windows'
            if current[platform]['state'] != 'succeeded':
                raise ValueError('Artifact build is not complete')
            path = directory / 'artifacts' / name
            expected = current[platform]['files'][name]
        elif action == 'receipt':
            path = directory / 'receipt.json'
            expected = common.describe(path)
        else:
            # Only a bounded tail; cannot be used to read arbitrary server files.
            path = common.safe(directory / 'windows.log')
            with path.open('rb') as log:
                log.seek(max(0, path.stat().st_size - 32768))
                return {'text': log.read().decode('utf-8', 'replace')}, None
        if common.describe(path) != expected:
            raise ValueError('Stored artifact/source integrity check failed')
        return {'file': expected}, path
    with common.locked(root / 'queue.lock', blocking=True):
        current = common.status(root, job)
        if action == 'complete-windows':
            if current['mode'] != common.WINDOWS_RELEASE_MODE:
                raise ValueError('This is not a Windows release job')
            files = current['windows'].get('files', {})
            names = {f'CView_{current["version"]}_{arch}-setup.exe' for arch in common.TARGETS}
            if current['windows']['state'] != 'succeeded' or set(files) != names:
                raise ValueError('Both Windows installers must succeed')
            if request.get('verified_windows') != files:
                raise ValueError('Release Mac must verify both Windows installer hashes and dependencies')
            for name, expected in files.items():
                if common.describe(directory / 'artifacts' / name) != expected:
                    raise ValueError('Stored artifact differs')
            receipt = directory / 'receipt.json'
            if not receipt.exists():
                common.write_json(receipt,
                                  {'job': job, 'version': current['version'], 'source': current['source'],
                                   'build_env': current.get('build_env', {}), 'files': files, 'finished': common.now(),
                                   'mode': common.WINDOWS_RELEASE_MODE, 'windows_signed': False,
                                   'windows_dependency_audit': 'passed-on-mac', 'installation_tested': False,
                                   'published': False})
            return common.status(root, job), None
        if current['mode'] != 'build-only':
            raise ValueError('macOS is handled by the formal release script for this job')
        mac = current['macos']
        if not request.get('token') or request['token'] != mac.get('token'):
            raise ValueError('This job belongs to another worker')
        if current['state'] == 'succeeded':
            return current, None
        if action == 'upload-macos':
            name = f'CView_{current["version"]}_arm64.dmg'
            expected = common.receive(stream, directory / 'artifacts' / name, request['file'])
            mac.pop('error', None)
            common.write_json(directory / 'macos.json',
                              {**mac, 'state': 'succeeded', 'finished': common.now(), 'files': {name: expected}})
        elif action == 'fail-macos':
            error = request.get('error', 'Mac worker failed')
            if not isinstance(error, str) or len(error) > 4096:
                raise ValueError('Invalid error text')
            common.write_json(directory / 'macos.json', {**mac, 'state': 'failed', 'error': error, 'finished': common.now()})
        elif action == 'complete':
            if any(current[p]['state'] != 'succeeded' for p in ('windows', 'macos')):
                raise ValueError('Both platforms must succeed')
            if request['verified_windows'] != current['windows']['files']:
                raise ValueError('Mac must verify both Windows installer hashes and dependencies')
            files = {**current['windows']['files'], **mac['files']}
            for name, expected in files.items():
                if common.describe(directory / 'artifacts' / name) != expected:
                    raise ValueError('Stored artifact differs')
            common.write_json(directory / 'receipt.json',
                              {'job': job, 'version': current['version'], 'source': current['source'], 'files': files,
                               'finished': common.now(), 'mode': 'build-only', 'macos_notarized': False,
                               'windows_signed': False, 'windows_dependency_audit': 'passed-on-mac',
                               'installation_tested': False, 'published': False})
        else:
            raise ValueError('Unsupported build action')
    return common.status(root, job), None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=Path(r'C:\CViewBuild'))
    args = parser.parse_args()
    try:
        if __import__('os').environ.get('SSH_ORIGINAL_COMMAND', '') != common.PROTOCOL:
            raise ValueError('Only the CView build protocol is allowed')
        raw = sys.stdin.buffer.readline(16385)
        if len(raw) > 16384 or not raw.endswith(b'\n'):
            raise ValueError('Invalid request header')
        request = json.loads(raw)
        if not isinstance(request, dict):
            raise ValueError('Invalid request')
        result, body = dispatch(common.safe(args.root), request, sys.stdin.buffer)
        sys.stdout.buffer.write((json.dumps({'ok': True, **result}) + '\n').encode('utf-8'))
        sys.stdout.buffer.flush()
        if body:
            with body.open('rb') as stream:
                shutil.copyfileobj(stream, sys.stdout.buffer)
    except Exception as error:
        sys.stdout.buffer.write((json.dumps({'ok': False, 'error': str(error)}) + '\n').encode('utf-8'))
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())

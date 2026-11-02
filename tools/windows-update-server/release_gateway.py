#!/usr/bin/env python3
"""Restricted SSH command. One bounded JSON header + optional binary body per call."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import tempfile
from contextlib import contextmanager

ROOT = Path(r'C:\CViewUpdates')
BASE = 'https://cv.dododo.app'
PROTOCOL = 'cview-release-v1'
# 앱이 검증하는 피드 서명. 게이트웨이가 이 기능을 알리지 않으면 클라이언트는 게시하지 않는다.
SIGNATURE_FORMAT = 'cview-update-signature-v1'
FEATURES = ['signed-feed-v1']


def version_tuple(version):
    if not isinstance(version, str) or not re.fullmatch(r'(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})', version):
        raise ValueError('Invalid stable version')
    return tuple(map(int, version.split('.')))


def safe(path):
    # Windows junctions are reparse points even when is_symlink() is false.
    if path.is_symlink() or (path.exists() and getattr(path.lstat(), 'st_file_attributes', 0) & 0x400):
        raise ValueError('Links/reparse points are not allowed')
    return path


def describe(path):
    safe(path)
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return {'size': path.stat().st_size, 'sha256': digest.hexdigest()}


def filenames(version):
    version_tuple(version)
    return [f'CView_{version}_arm64.dmg', f'CView_{version}_x64-setup.exe', 'notes.txt']


def latest(root):
    path = safe(root / 'site' / 'releases' / 'latest.json')
    return json.loads(path.read_text(encoding='utf-8')) if path.exists() else None


def require_new(root, version):
    previous = latest(root)
    if previous and version_tuple(version) <= version_tuple(previous['tag_name'].lstrip('v')):
        raise ValueError('Version must be newer than ' + previous['tag_name'])
    if (root / 'site' / 'releases' / version).exists():
        raise ValueError('Published releases are immutable')


@contextmanager
def locked(root):
    path = safe(root / '.release-automation.lock')
    with path.open('a+b') as stream:
        if stream.tell() == 0:
            stream.write(b'0')
            stream.flush()
        stream.seek(0)
        if os.name == 'nt':
            import msvcrt
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # Closing the descriptor releases the OS lock, including on process crash.
        yield


def receive(stream, target, size, expected):
    if type(size) is not int or not 0 < size <= 2 * 1024**3:
        raise ValueError('Invalid upload size')
    if not isinstance(expected, str) or not re.fullmatch('[0-9a-f]{64}', expected):
        raise ValueError('Invalid SHA-256')
    digest = hashlib.sha256()
    remaining = size
    with target.open('wb') as output:
        while remaining:
            chunk = stream.read(min(1024 * 1024, remaining))
            if not chunk:
                raise ValueError('Truncated upload')
            output.write(chunk)
            digest.update(chunk)
            remaining -= len(chunk)
        output.flush()
        os.fsync(output.fileno())
    if digest.hexdigest() != expected:
        raise ValueError('Upload SHA-256 mismatch')
    # Read back the actual stored bytes before acknowledging an upload.
    result = describe(target)
    if result != {'size': size, 'sha256': expected}:
        raise ValueError('Stored upload verification failed')
    return result


def expected_files(request):
    names = filenames(request['version'])
    expected = request['files']
    if not isinstance(expected, dict) or set(expected) != set(names):
        raise ValueError('Both installers and notes.txt are required')
    for name, item in expected.items():
        if (not isinstance(item, dict) or set(item) != {'size', 'sha256'}
                or type(item['size']) is not int or not 0 < item['size'] <= 2 * 1024**3
                or not isinstance(item['sha256'], str) or not re.fullmatch('[0-9a-f]{64}', item['sha256'])):
            raise ValueError('Invalid expected file metadata')
        if name == 'notes.txt' and item['size'] > 65536:
            raise ValueError('Release notes are too large')
    return expected


def same_release(manifest, version, expected):
    if manifest['tag_name'] != 'v' + version:
        return False
    actual = {a['name']: {'size': a['size'], 'sha256': a['sha256']} for a in manifest['assets']}
    body = manifest['body'].encode('utf-8')
    actual['notes.txt'] = {'size': len(body), 'sha256': hashlib.sha256(body).hexdigest()}
    if actual != expected:
        raise ValueError('Published version differs from these files; never overwrite it')
    return True


def signed_payload(manifest):
    """서명 대상: 버전·릴리스 페이지·노트 해시·자산(이름·크기·SHA-256·주소). 앱(updater.rs)과 같은 형식."""
    version = manifest['tag_name'].removeprefix('v')
    version_tuple(version)
    body = (manifest.get('body') or '').encode('utf-8')
    rows = [[SIGNATURE_FORMAT], ['version', version], ['release', manifest['html_url']],
            ['notes', hashlib.sha256(body).hexdigest()]]
    for asset in sorted(manifest['assets'], key=lambda a: a['name']):
        if type(asset['size']) is not int or asset['size'] <= 0:
            raise ValueError('Invalid asset size')
        rows.append(['asset', asset['name'], str(asset['size']), asset['sha256'], asset['browser_download_url']])
    # 필드 안의 공백·제어 문자는 줄·필드 경계를 흐리므로 받지 않는다(앱도 같은 규칙).
    if any(not isinstance(f, str) or not f or not f.isprintable() or any(c.isspace() for c in f)
           for row in rows for f in row):
        raise ValueError('Signed fields must be nonempty without whitespace')
    lines = [' '.join(row) for row in rows]
    return ('\n'.join(lines) + '\n').encode('utf-8')


def checked_signature(value):
    """서명 형식만 본다(서버에는 비밀 키도 검증 라이브러리도 없다). 검증은 클라이언트와 앱이 한다."""
    if (not isinstance(value, dict) or set(value) != {'alg', 'key_id', 'value'} or value['alg'] != 'ed25519'
            or not isinstance(value['key_id'], str) or not re.fullmatch('[0-9a-f]{16}', value['key_id'])
            or not isinstance(value['value'], str) or not re.fullmatch('[A-Za-z0-9+/]{86}==', value['value'])):
        raise ValueError('A valid ed25519 feed signature is required')
    return {'alg': 'ed25519', 'key_id': value['key_id'], 'value': value['value']}


def dispatch(request, stream, root=ROOT):
    action = request.get('action')
    if action not in {'preflight', 'probe', 'upload', 'validate', 'publish'}:
        raise ValueError('Unsupported release action')
    safe(root)
    incoming = safe(root / 'incoming')
    if not incoming.is_dir():
        raise ValueError('Upload directory is unavailable')
    for name in ['publish.py', 'check_server.py']:
        if not safe(root / 'tools' / name).is_file():
            raise ValueError('Missing server tool: ' + name)
    with locked(root):
        if action == 'preflight':
            return {'protocol': PROTOCOL, 'features': FEATURES, 'hostname': socket.gethostname(),
                    'latest': latest(root), 'free_bytes': shutil.disk_usage(incoming).free}
        if action == 'probe':
            if request.get('size') != 1024 * 1024:
                raise ValueError('Probe must contain exactly 1 MiB')
            with tempfile.TemporaryDirectory(prefix='.automation-probe-', dir=incoming) as tmp:
                info = receive(stream, Path(tmp) / 'probe.bin', request['size'], request['sha256'])
            return {'probe': info, 'cleaned': True}
        version = request['version']
        names = filenames(version)
        if action == 'upload':
            require_new(root, version)
            name = request['name']
            if name not in names:
                raise ValueError('Filename is not a release artifact')
            if name == 'notes.txt' and request['size'] > 65536:
                raise ValueError('Release notes are too large')
            folder = safe(incoming / version)
            folder.mkdir(exist_ok=True)
            destination = safe(folder / name)
            with tempfile.TemporaryDirectory(prefix='.upload-', dir=folder) as tmp:
                temporary = Path(tmp) / name
                info = receive(stream, temporary, request['size'], request['sha256'])
                if destination.exists():
                    if describe(destination) != info:
                        raise ValueError('Staged file differs; inspect it before removing/replacing it')
                else:
                    os.replace(temporary, destination)
            return {'uploaded': name, **info}
        expected = expected_files(request)
        signature = checked_signature(request.get('signature'))
        current = latest(root)
        reused = bool(current and same_release(current, version, expected))
        if reused and current.get('signature') != signature:
            raise ValueError('Published signature differs; never overwrite it')
        if not reused:
            require_new(root, version)
            folder = safe(incoming / version)
            for name in names:
                if describe(safe(folder / name)) != expected[name]:
                    raise ValueError('Staged file mismatch: ' + name)
            notes = (folder / 'notes.txt').read_bytes().decode('utf-8')
            if action == 'validate':
                return {'validated': version, 'published': False}
            # Fixed, administrator-owned module; no client-supplied code or paths.
            sys.path.insert(0, str(root / 'tools'))
            from publish import publish
            current = publish(root / 'site', BASE, version, [folder / n for n in names[:2]], notes, signature)
            if not same_release(current, version, expected) or current.get('signature') != signature:
                raise ValueError('Published manifest mismatch')
        if action == 'validate':
            return {'validated': version, 'published': True, 'reused': True}
        check = subprocess.run([sys.executable, '-I', '-B', str(root / 'tools' / 'check_server.py'),
                                '--base-url', BASE, '--download'], capture_output=True, timeout=600)
        if check.returncode:
            raise RuntimeError('Published, but server download verification failed: ' + check.stderr.decode('utf-8', 'replace'))
        return {'published': True, 'version': version, 'reused': reused,
                'server_verified': True, 'manifest': current}


def main():
    try:
        if os.environ.get('SSH_ORIGINAL_COMMAND') != PROTOCOL:
            raise ValueError('This key permits only the CView release protocol')
        header = sys.stdin.buffer.readline(8193)
        if len(header) > 8192 or not header.endswith(b'\n'):
            raise ValueError('Invalid request header')
        result = dispatch(json.loads(header), sys.stdin.buffer)
        print(json.dumps({'ok': True, **result}, ensure_ascii=True), flush=True)
    except Exception as error:
        print(json.dumps({'ok': False, 'error': str(error)}, ensure_ascii=True), flush=True)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())

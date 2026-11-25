#!/usr/bin/env python3
"""Publish a static, GitHub-compatible CView update feed. Python standard library only."""
import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import quote, urlsplit, urlunsplit

STABLE_VERSION_RE = re.compile(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)')


def public_base(value):
    url = urlsplit(value)
    if (url.scheme != 'https' or not url.hostname or url.username is not None
            or url.password is not None or url.query or url.fragment):
        raise ValueError('Base URL must be HTTPS without credentials, query or fragment')
    return urlunsplit(url).rstrip('/')


def _is_link(path):
    return path.is_symlink() or (path.exists() and getattr(path.lstat(), 'st_file_attributes', 0) & 0x400)


def _sha256_file(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_write(path, data):
    path = Path(path)
    if _is_link(path) or _is_link(path.parent):
        raise ValueError(f'Refusing linked destination: {path}')
    descriptor, temporary = tempfile.mkstemp(prefix='.publish-', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as out:
            out.write(data)
            if os.name != 'nt':
                os.fchmod(out.fileno(), 0o644)
            out.flush()
            os.fsync(out.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def build_release_catalog(root):
    """Read immutable release manifests under root/releases in descending semantic version order."""
    releases = Path(root) / 'releases'
    if not releases.is_dir():
        return b'[]\n'
    catalog = []
    for folder in sorted(releases.iterdir()):
        if not folder.is_dir() or _is_link(folder) or not STABLE_VERSION_RE.fullmatch(folder.name):
            continue
        manifest_path = folder / 'manifest.json'
        if not manifest_path.is_file() or _is_link(manifest_path):
            continue
        value = json.loads(manifest_path.read_text(encoding='utf-8-sig'))
        if value.get('tag_name') != 'v' + folder.name or not isinstance(value.get('assets'), list):
            raise ValueError(f'Invalid release manifest: {folder.name}')
        catalog.append(value)
    catalog.sort(key=lambda item: tuple(map(int, item['tag_name'][1:].split('.'))), reverse=True)
    return (json.dumps(catalog, ensure_ascii=False, indent=2) + '\n').encode('utf-8')


def _verify_promoted_release(destination, release_url, version, sources, notes, signature):
    """Return (manifest, encoded_bytes) if destination is an exact already-promoted copy of this release."""
    manifest_path = destination / 'manifest.json'
    index_path = destination / 'index.html'
    if (not destination.is_dir() or _is_link(destination)
            or not manifest_path.is_file() or _is_link(manifest_path)
            or not index_path.is_file() or _is_link(index_path)):
        return None
    try:
        raw = manifest_path.read_bytes()
        manifest = json.loads(raw.decode('utf-8'))
    except (OSError, UnicodeDecodeError, ValueError):
        return None
    if (manifest.get('tag_name') != 'v' + version
            or manifest.get('name') != f'CView {version}'
            or manifest.get('html_url') != release_url
            or manifest.get('body') != notes):
        return None
    if signature is None:
        if 'signature' in manifest:
            return None
    elif manifest.get('signature') != signature:
        return None
    expected_assets = []
    for source in sources:
        placed = destination / source.name
        sums = destination / f'{source.name}.sha256'
        if (not placed.is_file() or _is_link(placed)
                or not sums.is_file() or _is_link(sums)):
            return None
        source_hash = _sha256_file(source)
        if placed.stat().st_size != source.stat().st_size or _sha256_file(placed) != source_hash:
            return None
        if sums.read_text(encoding='utf-8') != f'{source_hash}  {source.name}\n':
            return None
        expected_assets.append({
            'name': source.name,
            'size': source.stat().st_size,
            'browser_download_url': release_url + quote(source.name),
            'sha256': source_hash,
        })
    if manifest.get('assets') != expected_assets:
        return None
    return manifest, raw


@contextmanager
def _publish_lock(root):
    """OS kernel file lock: prevents concurrent publishes while automatically releasing on process crash."""
    lock = root.parent / f'.{root.name}-publish.lock'
    if _is_link(lock):
        raise ValueError('Refusing linked publish lock')
    with lock.open('a+b') as stream:
        if stream.tell() == 0:
            stream.write(b'0')
            stream.flush()
        stream.seek(0)
        if os.name == 'nt':
            import msvcrt
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        stream.seek(0)
        stream.truncate()
        stream.write(f'{os.getpid()}\n'.encode('ascii'))
        stream.flush()
        yield


def _publish(root, base_url, version, artifacts, notes='', signature=None):
    base_url = public_base(base_url)
    if not STABLE_VERSION_RE.fullmatch(version):
        raise ValueError('Use a stable version such as 0.1.1')
    sources = [Path(p).resolve(strict=True) for p in artifacts]
    if not sources or len({p.name.casefold() for p in sources}) != len(sources):
        raise ValueError('Provide at least one artifact; filenames must be unique ignoring case')
    for path in sources:
        if (not path.is_file() or path.stat().st_size == 0
                or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,179}', path.name)
                or not path.name.lower().endswith(('.dmg', '.zip', '.msi', '.exe', '.tar.gz'))
                or re.search(r'[_-](local|source)([_.-]|$)', path.name, re.I)):
            raise ValueError(f'Invalid distribution artifact (local/source builds excluded): {path.name}')
    root = Path(root).resolve()
    releases = root / 'releases'
    releases.mkdir(parents=True, exist_ok=True)
    latest = releases / 'latest.json'
    if latest.exists():
        previous = json.loads(latest.read_text(encoding='utf-8'))['tag_name'].lstrip('vV')
        if tuple(map(int, version.split('.'))) <= tuple(map(int, previous.split('.'))):
            raise ValueError('New version must be newer than the published release')
    release_url = f'{base_url}/releases/{version}/'
    destination = releases / version
    if destination.exists():
        # 버전 폴더 배치 직후·latest.json 교체 직전에 비정상 종료된 경우:
        # 이미 놓인 파일·manifest·서명이 요청과 완전히 일치할 때만 피드·카탈로그 갱신부터 이어 간다.
        resumed = _verify_promoted_release(destination, release_url, version, sources, notes, signature)
        if resumed is None:
            raise FileExistsError(f'Release {version} already exists; releases are immutable')
        manifest, encoded = resumed
        atomic_write(latest, encoded)
        atomic_write(root / 'release-catalog.json', build_release_catalog(root))
        return manifest

    with tempfile.TemporaryDirectory(prefix='cview-publish-', dir=root.parent) as temporary:
        stage = Path(temporary) / version
        stage.mkdir()
        assets = []
        for source in sources:
            target = stage / source.name
            shutil.copyfile(source, target)
            checksum = _sha256_file(target)
            (stage / f'{source.name}.sha256').write_text(f'{checksum}  {source.name}\n', encoding='utf-8')
            assets.append({'name': source.name, 'size': target.stat().st_size,
                           'browser_download_url': release_url + quote(source.name),
                           'sha256': checksum})
        manifest = {'tag_name': 'v' + version, 'name': f'CView {version}', 'body': notes,
                    'html_url': release_url, 'published_at': datetime.now(timezone.utc).isoformat(),
                    'assets': assets}
        if signature:
            # 릴리스 Mac이 서명한 값(release_gateway.signed_payload). 앱은 이것 없이 자동 설치하지 않는다.
            manifest['signature'] = signature
        encoded = (json.dumps(manifest, ensure_ascii=False, indent=2) + '\n').encode('utf-8')
        (stage / 'manifest.json').write_bytes(encoded)
        links = ''.join(f'<li><a href="{html.escape(asset["browser_download_url"], quote=True)}">'
                        f'{html.escape(asset["name"])}</a> ({asset["size"]:,} bytes) · '
                        f'<a href="{html.escape(asset["browser_download_url"], quote=True)}.sha256">SHA-256</a></li>'
                        for asset in assets)
        page = (f'<!doctype html><html lang="ko"><meta charset="utf-8">'
                f'<meta name="viewport" content="width=device-width, initial-scale=1">'
                f'<title>CView {version}</title><link rel="stylesheet" href="../../style.css">'
                f'<main><h1>CView {version}</h1><p>운영체제와 칩 종류에 맞는 설치 파일을 선택하세요.</p>'
                f'<ul>{links}</ul><h2>변경 사항</h2><pre>{html.escape(notes)}</pre></main></html>')
        (stage / 'index.html').write_text(page, encoding='utf-8')
        # Staging inherits the admin-only parent ACL on Windows. A rename on
        # the same volume preserves it, so grant IIS read access before promotion.
        if os.name == 'nt':
            subprocess.run(['icacls.exe', str(stage), '/grant',
                            '*S-1-5-32-568:(OI)(CI)(RX)', '/T', '/Q'],
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        # Promote a complete immutable directory, then atomically change the feed and release catalog.
        os.rename(stage, destination)
        try:
            atomic_write(latest, encoded)
        except OSError:
            shutil.rmtree(destination)
            raise
        atomic_write(root / 'release-catalog.json', build_release_catalog(root))
    return manifest


def publish(root, base_url, version, artifacts, notes='', signature=None):
    root = Path(root).resolve()
    root.parent.mkdir(parents=True, exist_ok=True)
    with _publish_lock(root):
        return _publish(root, base_url, version, artifacts, notes, signature)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', required=True, type=Path, help='IIS public site directory')
    parser.add_argument('--base-url', required=True, help='Public HTTPS origin, optionally with a path prefix')
    parser.add_argument('--version', required=True, help='Stable version: 0.1.1')
    parser.add_argument('--artifact', required=True, action='append', type=Path)
    parser.add_argument('--notes-file', type=Path)
    args = parser.parse_args()
    try:
        result = publish(args.root, args.base_url, args.version, args.artifact,
                         args.notes_file.read_text(encoding='utf-8') if args.notes_file else '')
    except (OSError, ValueError) as error:
        parser.exit(1, f'Publish failed: {error}\n')
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()

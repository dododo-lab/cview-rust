#!/usr/bin/env python3
"""Publish a static, GitHub-compatible CView update feed. Python standard library only."""
import argparse
from datetime import datetime, timezone
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shutil
import tempfile
from urllib.parse import quote, urlsplit, urlunsplit


def public_base(value):
    url = urlsplit(value)
    if (url.scheme != 'https' or not url.hostname or url.username is not None
            or url.password is not None or url.query or url.fragment):
        raise ValueError('Base URL must be HTTPS without credentials, query or fragment')
    return urlunsplit(url).rstrip('/')


def atomic_write(path, data):
    path = Path(path)
    descriptor, temporary = tempfile.mkstemp(prefix='.publish-', suffix='.tmp', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as out:
            out.write(data)
            out.flush()
            os.fsync(out.fileno())
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def _publish(root, base_url, version, artifacts, notes=''):
    base_url = public_base(base_url)
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version):
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
    destination = releases / version
    if destination.exists():
        raise FileExistsError(f'Release {version} already exists; releases are immutable')
    latest = releases / 'latest.json'
    if latest.exists():
        previous = json.loads(latest.read_text(encoding='utf-8'))['tag_name'].lstrip('vV')
        if tuple(map(int, version.split('.'))) <= tuple(map(int, previous.split('.'))):
            raise ValueError('New version must be newer than the published release')
    release_url = f'{base_url}/releases/{version}/'
    with tempfile.TemporaryDirectory(prefix='cview-publish-', dir=root.parent) as temporary:
        stage = Path(temporary) / version
        stage.mkdir()
        assets = []
        for source in sources:
            target = stage / source.name
            shutil.copyfile(source, target)
            digest = hashlib.sha256()
            with target.open('rb') as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(chunk)
            checksum = digest.hexdigest()
            (stage / f'{source.name}.sha256').write_text(f'{checksum}  {source.name}\n', encoding='utf-8')
            assets.append({'name': source.name, 'size': target.stat().st_size,
                           'browser_download_url': release_url + quote(source.name),
                           'sha256': checksum})
        manifest = {'tag_name': 'v' + version, 'name': f'CView {version}', 'body': notes,
                    'html_url': release_url, 'published_at': datetime.now(timezone.utc).isoformat(),
                    'assets': assets}
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
        # Promote a complete immutable directory, then atomically change the feed.
        os.rename(stage, destination)
        try:
            atomic_write(releases / 'latest.json', encoded)
        except OSError:
            shutil.rmtree(destination)
            raise
    return manifest


def publish(root, base_url, version, artifacts, notes=''):
    root = Path(root).resolve()
    root.parent.mkdir(parents=True, exist_ok=True)
    lock = root.parent / f'.{root.name}-publish.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    try:
        with os.fdopen(descriptor, 'w') as out:
            out.write(str(os.getpid()))
        return _publish(root, base_url, version, artifacts, notes)
    finally:
        lock.unlink(missing_ok=True)


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

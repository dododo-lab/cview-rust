#!/usr/bin/env python3
"""Mirror an already-published CView release (update server) to GitHub Releases and verify it.

The GitHub copy uses the exact files listed in the signed per-version manifest
(https://cv.dododo.app/releases/<version>/manifest.json) and adds `latest.json`: the same feed with
GitHub addresses, signed here with the local feed signing key. Apps read it from
https://github.com/dododo-lab/cview-rust/releases/latest/download/latest.json. Nothing here builds or
notarizes, and no secret goes to GitHub.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.request

REPO = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location('publish_update', REPO / 'scripts' / 'publish-update.py')
client = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(client)
BASE, describe, installers, version_tuple = client.BASE, client.describe, client.installers, client.version_tuple
# 앱 updater 의 기본 피드(system.rs DEFAULT_UPDATE_FEED)와 같은 저장소여야 한다.
GITHUB = 'https://github.com/dododo-lab/cview-rust'
FEED_NAME = 'latest.json'


def sums_name(version):
    return f'CView_{version}_SHA256SUMS.txt'


def sums_text(assets):
    return ''.join(f'{a["sha256"]}  {a["name"]}\n' for a in assets)


def server_manifest(version):
    """게시된 버전의 서명된 manifest — 앱이 믿는 것과 같은 키로 확인한다."""
    with client.get(f'{BASE}/releases/{version}/manifest.json') as response:
        manifest = json.load(response)
    if manifest.get('tag_name') != 'v' + version or manifest.get('html_url') != f'{BASE}/releases/{version}/':
        raise ValueError('Update server manifest does not describe v' + version)
    names = sorted(a['name'] for a in manifest['assets'])
    if names not in client.published_installer_sets(version, True):
        raise ValueError('Update server manifest must list exactly the platform installers')
    for asset in manifest['assets']:
        if asset['browser_download_url'] != f'{BASE}/releases/{version}/{asset["name"]}':
            raise ValueError('Unexpected artifact URL: ' + asset['name'])
    client.verify_signature(manifest)
    return manifest


def github_feed(manifest):
    """GitHub 피드(서명 전) — 서버 manifest 와 같은 버전·노트·파일 크기·SHA-256, 주소만 GitHub."""
    tag = manifest['tag_name']
    feed = {k: v for k, v in manifest.items() if k != 'signature'}
    feed['html_url'] = f'{GITHUB}/releases/tag/{tag}'
    feed['assets'] = [{**a, 'browser_download_url': f'{GITHUB}/releases/download/{tag}/{a["name"]}'}
                      for a in manifest['assets']]
    return feed


def feed_bytes(feed):
    return (json.dumps(feed, ensure_ascii=False, indent=2) + '\n').encode('utf-8')


def check_github_feed(feed, manifest):
    """GitHub 에 올라간 피드가 서버 manifest 와 같은 릴리스이고 앱이 믿는 키로 서명됐는가."""
    if {k: v for k, v in feed.items() if k != 'signature'} != github_feed(manifest):
        raise ValueError('GitHub latest.json differs from the update server release')
    client.verify_signature(feed)


def download(url, target, expected):
    digest = hashlib.sha256()
    size = 0
    with client.get(url) as response, target.open('wb') as stream:
        while chunk := response.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
            stream.write(chunk)
    if size != expected['size'] or digest.hexdigest() != expected['sha256']:
        raise ValueError('Update server download mismatch: ' + target.name)


def collect(manifest, dist, tmp):
    """dist/ 의 파일이 게시본과 같으면 쓰고, 없으면 업데이트 서버에서 받는다. 다른 파일이면 멈춘다."""
    paths = []
    for asset in manifest['assets']:
        local = dist / asset['name']
        if local.is_file():
            if describe(local) != {'size': asset['size'], 'sha256': asset['sha256']}:
                raise ValueError(f'{local} differs from the published file; refusing to mirror it')
            paths.append(local)
        else:
            print('Downloading from update server: ' + asset['name'], flush=True)
            target = tmp / asset['name']
            download(asset['browser_download_url'], target, asset)
            paths.append(target)
    return paths


def release_notes(manifest):
    version = manifest['tag_name'][1:]
    return (manifest['body'].rstrip() + '\n\n---\n'
            f'업데이트 서버(앱 자동 업데이트 원본): {manifest["html_url"]}\n'
            '여기 첨부한 설치 파일은 업데이트 서버에 게시된 파일과 같습니다. '
            f'`{sums_name(version)}`로 SHA-256을 확인할 수 있습니다.\n\n'
            '```\n' + sums_text(manifest['assets']) + '```\n')


def gh(*args, capture=True):
    result = subprocess.run(['gh', *args], capture_output=capture, text=True)
    if result.returncode:
        raise RuntimeError(f'gh {" ".join(args[:2])} failed: {(result.stderr or "").strip()}')
    return result.stdout


def github_release(tag):
    result = subprocess.run(['gh', 'release', 'view', tag, '--json', 'isDraft,assets,url'],
                            capture_output=True, text=True)
    if result.returncode:
        if 'not found' in result.stderr.lower():
            return None
        raise RuntimeError('gh release view failed: ' + result.stderr.strip())
    return json.loads(result.stdout)


def expected_assets(manifest):
    version = manifest['tag_name'][1:]
    sums = sums_text(manifest['assets']).encode('utf-8')
    expected = {a['name']: {'size': a['size'], 'sha256': a['sha256']} for a in manifest['assets']}
    expected[sums_name(version)] = {'size': len(sums), 'sha256': hashlib.sha256(sums).hexdigest()}
    return expected


def compare_assets(release, expected):
    """GitHub 자산 목록 대조. 이미 있는 이름이 다른 내용이면 덮어쓰지 않고 실패한다."""
    missing = set(expected)
    for asset in release['assets']:
        name = asset['name']
        if name not in expected:
            raise ValueError('Unexpected GitHub release asset: ' + name)
        want = expected[name]
        if asset.get('state') not in (None, 'uploaded') or asset['size'] != want['size'] or \
                asset.get('digest') not in (None, 'sha256:' + want['sha256']):
            raise ValueError(f'GitHub asset {name} differs from the published file; not overwriting')
        missing.discard(name)
    return sorted(missing)


def verify_downloads(tag, expected):
    """GitHub 에서 전체를 다시 받아 크기·SHA-256을 확인한다(초안이어도 권한이 있으면 받을 수 있다)."""
    with tempfile.TemporaryDirectory(prefix='cview-github-verify-') as tmp:
        gh('release', 'download', tag, '--dir', tmp)
        got = {path.name: describe(path) for path in Path(tmp).iterdir()}
    if got != expected:
        raise ValueError('GitHub download mismatch: ' + ', '.join(sorted(set(got) ^ set(expected)) or
                         [name for name in expected if got.get(name) != expected[name]]))


def verify_public_feed(expected):
    """앱이 받는 주소(…/releases/latest/download/latest.json, GitHub 리다이렉트 포함)가 이 릴리스의 피드인가."""
    request = urllib.request.Request(f'{GITHUB}/releases/latest/download/{FEED_NAME}',
                                     headers={'User-Agent': 'CView-Release-Automation', 'Cache-Control': 'no-cache'})
    with urllib.request.urlopen(request, timeout=60) as response:
        if not response.geturl().startswith('https://'):
            raise ValueError('GitHub feed redirected off HTTPS')
        data = response.read()
    if {'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()} != expected:
        raise ValueError('GitHub latest feed is not this release; check the "Latest" release on GitHub')


def summary(lines):
    path = os.environ.get('GITHUB_STEP_SUMMARY')
    if path:
        with open(path, 'a', encoding='utf-8') as stream:
            stream.write('\n'.join(lines) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--version', required=True, help='Version already published to the update server')
    parser.add_argument('--dist', type=Path, default=REPO / 'dist')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--draft', action='store_true', help='Upload and verify, but leave a new release as a draft')
    mode.add_argument('--verify-only', action='store_true', help='Never upload; check the GitHub release against the server')
    args = parser.parse_args()
    version = args.version.removeprefix('v')
    tag = 'v' + version
    try:
        version_tuple(version)
        manifest = server_manifest(version)
        expected = expected_assets(manifest)
        release = github_release(tag)
        if args.verify_only:
            if release is None:
                raise ValueError('No GitHub release for ' + tag)
            has_feed = any(a['name'] == FEED_NAME for a in release['assets'])
            if has_feed:
                with tempfile.TemporaryDirectory(prefix='cview-github-feed-') as tmp:
                    gh('release', 'download', tag, '--pattern', FEED_NAME, '--dir', tmp)
                    feed_file = Path(tmp) / FEED_NAME
                    check_github_feed(json.loads(feed_file.read_text(encoding='utf-8')), manifest)
                    expected[FEED_NAME] = describe(feed_file)
            else:
                # 피드를 넣기 전에 게시된 릴리스(v0.1.73 이하). 앱은 이때 자체 서버 피드로 넘어간다.
                print(f'Warning: {tag} has no {FEED_NAME}; apps fall back to the update server feed')
            missing = compare_assets(release, expected)
            if missing:
                raise ValueError('GitHub release is missing ' + ', '.join(missing))
            verify_downloads(tag, expected)
            if has_feed and not release['isDraft'] and client.verify_https()['tag_name'] == tag:
                verify_public_feed(expected[FEED_NAME])
            print(f'Verified GitHub release {release["url"]} against the signed update server manifest')
            summary([f'### CView {version} GitHub 릴리스 검증', '', f'- 릴리스: {release["url"]}',
                     f'- 기준: {BASE}/releases/{version}/manifest.json (ed25519 서명 확인)', '',
                     '| 파일 | 크기 | SHA-256 |', '| --- | ---: | --- |'] +
                    [f'| `{n}` | {e["size"]} | `{e["sha256"]}` |' for n, e in expected.items()])
            return 0
        # 태그는 release-macos.sh 가 업데이트 서버 게시 뒤에 푸시한다. 없는 태그로 새로 만들지 않는다.
        if subprocess.run(['git', 'ls-remote', '--exit-code', '--tags', 'origin', f'refs/tags/{tag}'],
                          capture_output=True).returncode:
            raise ValueError(f'Tag {tag} is not on origin; push it first (git push origin {tag})')
        with tempfile.TemporaryDirectory(prefix='cview-github-release-') as tmp:
            tmp = Path(tmp)
            sums = tmp / sums_name(version)
            sums.write_text(sums_text(manifest['assets']), encoding='utf-8')
            notes = tmp / 'notes.md'
            notes.write_text(release_notes(manifest), encoding='utf-8')
            # Ed25519 서명은 결정적이라 다시 실행해도 같은 바이트다(이미 올린 latest.json 과 비교할 수 있다).
            feed = github_feed(manifest)
            feed['signature'] = client.sign_manifest(feed)
            feed_file = tmp / FEED_NAME
            feed_file.write_bytes(feed_bytes(feed))
            expected[FEED_NAME] = describe(feed_file)
            if release is None:
                print('Creating draft GitHub release ' + tag, flush=True)
                gh('release', 'create', tag, '--verify-tag', '--draft', '--title', manifest['name'],
                   '--notes-file', str(notes))
                release = github_release(tag)
            missing = compare_assets(release, expected)
            if missing and not release['isDraft']:
                # immutable releases: 게시된 릴리스에는 파일을 더할 수 없다. 그래서 초안에 모두 올린 뒤 게시한다.
                raise ValueError(f'{tag} is already published (immutable) without ' + ', '.join(missing))
            if missing:
                files = {p.name: p for p in collect(manifest, args.dist, tmp)}
                files[sums.name] = sums
                files[FEED_NAME] = feed_file
                print('Uploading ' + ', '.join(missing), flush=True)
                gh('release', 'upload', tag, *[str(files[name]) for name in missing])
                release = github_release(tag)
                if compare_assets(release, expected):
                    raise ValueError('GitHub release is still missing assets after upload')
        verify_downloads(tag, expected)
        latest = client.verify_https()['tag_name'] == tag
        if release['isDraft'] and not args.draft:
            gh('release', 'edit', tag, '--draft=false', f'--latest={str(latest).lower()}')
            release = github_release(tag)
        if latest and not release['isDraft']:
            verify_public_feed(expected[FEED_NAME])
        state = 'draft' if release['isDraft'] else 'published'
        args.dist.mkdir(exist_ok=True)
        receipt = args.dist / f'CView_{version}_github-receipt.json'
        receipt.write_text(json.dumps({'verified_at': datetime.now(timezone.utc).isoformat(), 'url': release['url'],
                                       'state': state, 'assets': expected}, ensure_ascii=False, indent=2) + '\n',
                           encoding='utf-8')
        print(f'GitHub release {state} and download-verified: {release["url"]}')
        print('Receipt: ' + str(receipt))
        return 0
    except (OSError, ValueError, RuntimeError, KeyError, subprocess.SubprocessError) as error:
        print(f'GitHub release mirror failed: {error}', file=sys.stderr)
        print('The update server release is unaffected. Fix the cause and rerun the same command; '
              'existing GitHub assets are never overwritten.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())

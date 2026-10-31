#!/usr/bin/env python3
"""Upload both CView installers, publish atomically, and verify HTTPS downloads."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / 'tools' / 'windows-update-server'))
from release_gateway import BASE, PROTOCOL, describe, filenames, same_release, version_tuple


# Server address, host name and pinned host key live outside the public repository.
# Default: ~/.config/cview-release/{server.json,known_hosts}; environment variables override.
CONFIG_DIR = Path(os.environ.get('CVIEW_RELEASE_CONFIG_DIR', '~/.config/cview-release')).expanduser()
SETUP_HINT = 'See tools/windows-update-server/AUTOMATION.md (local release server settings)'


def server_settings():
    path = CONFIG_DIR / 'server.json'
    try:
        data = json.loads(path.read_text(encoding='utf-8'))
    except FileNotFoundError:
        data = {}
    except (ValueError, UnicodeError) as error:
        raise ValueError(f'Invalid release server settings {path}: {error}') from None
    target = os.environ.get('CVIEW_RELEASE_SSH_TARGET') or data.get('ssh_target')
    name = os.environ.get('CVIEW_RELEASE_SERVER_NAME') or data.get('server_name')
    known_hosts = Path(os.environ.get('CVIEW_RELEASE_KNOWN_HOSTS') or CONFIG_DIR / 'known_hosts').expanduser()
    if not target or not name:
        raise ValueError(f'Release server is not configured: {path} needs ssh_target and server_name. {SETUP_HINT}')
    if target.startswith('-') or any(c.isspace() for c in target):
        raise ValueError('Invalid release SSH target')
    if not known_hosts.is_file():
        raise ValueError(f'Pinned release host key is missing: {known_hosts}. {SETUP_HINT}')
    return {'ssh_target': target, 'server_name': name, 'known_hosts': known_hosts}


def ssh_command(settings=None):
    settings = settings or server_settings()
    key = Path(os.environ.get('CVIEW_RELEASE_SSH_KEY', '~/.ssh/cview_release_ed25519')).expanduser()
    if not key.is_file():
        raise ValueError(f'Release key is missing: {key}. See tools/windows-update-server/AUTOMATION.md')
    if os.name != 'nt' and key.stat().st_mode & 0o077:
        raise ValueError(f'Release key must be private (chmod 600): {key}')
    null = os.devnull
    return ['ssh', '-F', null, '-T', '-i', str(key),
            '-o', 'BatchMode=yes', '-o', 'IdentitiesOnly=yes', '-o', 'PreferredAuthentications=publickey',
            '-o', 'ControlMaster=no', '-o', 'ControlPath=none', '-o', 'StrictHostKeyChecking=yes',
            '-o', 'UserKnownHostsFile=' + str(settings['known_hosts']),
            '-o', 'GlobalKnownHostsFile=' + null, '-o', 'HostKeyAlgorithms=ssh-ed25519',
            '-o', 'ConnectTimeout=10', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3',
            '--', settings['ssh_target'], PROTOCOL]


def rpc(request, body=None):
    # A temporary stream avoids buffering large installers in RAM and handles SSH pipe failures.
    with tempfile.TemporaryFile() as stream:
        stream.write(json.dumps(request).encode('ascii') + b'\n')
        if body:
            with Path(body).open('rb') as source:
                shutil.copyfileobj(source, stream)
        stream.seek(0)
        result = subprocess.run(ssh_command(), stdin=stream, capture_output=True, timeout=900)
    try:
        response = json.loads(result.stdout)
    except (ValueError, UnicodeError):
        raise RuntimeError('SSH release connection failed: ' + result.stderr.decode('utf-8', 'replace').strip()) from None
    if result.returncode or not response.get('ok'):
        raise RuntimeError(response.get('error', 'SSH release request failed'))
    return response


class SameOriginRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not newurl.startswith(BASE + '/'):
            raise ValueError('Unexpected release redirect')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def get(url):
    if not url.startswith(BASE + '/'):
        raise ValueError('Unexpected release URL')
    opener = urllib.request.build_opener(SameOriginRedirect())
    return opener.open(urllib.request.Request(url, headers={'Cache-Control': 'no-cache',
                       'User-Agent': 'CView-Release-Automation'}), timeout=60)


def verify_https(version=None, expected=None, download=False):
    with get(BASE + '/health.json') as response:
        if json.load(response).get('status') != 'ok':
            raise ValueError('HTTPS health check failed')
    with get(BASE + '/releases/latest.json') as response:
        manifest = json.load(response)
    if version and not same_release(manifest, version, expected):
        raise ValueError('HTTPS latest feed is not the requested release')
    public_version = manifest['tag_name'].removeprefix('v')
    version_tuple(public_version)
    if manifest['html_url'] != f'{BASE}/releases/{public_version}/':
        raise ValueError('Unexpected release page URL')
    with get(manifest['html_url']) as response:
        if response.status != 200:
            raise ValueError('Release page is unavailable')
    names = filenames(public_version)[:2]
    if sorted(a['name'] for a in manifest['assets']) != sorted(names):
        raise ValueError('The feed must contain exactly the two platform installers')
    if download:
        for asset in manifest['assets']:
            url = f'{BASE}/releases/{public_version}/{asset["name"]}'
            if asset['browser_download_url'] != url:
                raise ValueError('Unexpected artifact URL')
            digest = hashlib.sha256()
            size = 0
            with get(url) as response:
                while chunk := response.read(1024 * 1024):
                    digest.update(chunk)
                    size += len(chunk)
            if size != asset['size'] or digest.hexdigest() != asset['sha256']:
                raise ValueError('HTTPS download mismatch: ' + asset['name'])
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--preflight', action='store_true', help='Check key authentication and HTTPS without uploading')
    parser.add_argument('--probe', action='store_true', help='Write/read/delete a 1 MiB probe and verify current downloads; never publish')
    parser.add_argument('--version', help='Exact already-built stable version')
    parser.add_argument('--notes-file', type=Path, help='UTF-8 release notes, preserved for retries')
    parser.add_argument('--dist', type=Path, default=REPO / 'dist')
    parser.add_argument('--validate-only', action='store_true', help='Upload and hash-check the real artifacts without publishing')
    parser.add_argument('--allow-unnotarized', action='store_true', help='Only with explicit user authorization; adds disclosure')
    args = parser.parse_args()
    try:
        expected_server = server_settings()['server_name']
        state = rpc({'action': 'preflight'})
        if state['protocol'] != PROTOCOL or state['hostname'].upper() != expected_server.upper():
            raise ValueError('Unexpected release server')
        current = verify_https()
        if current != state['latest']:
            raise ValueError('SSH and HTTPS manifests disagree; retry after concurrent publishing finishes')
        if args.preflight and not args.probe:
            if args.version and version_tuple(args.version) <= version_tuple(current['tag_name'].lstrip('v')):
                raise ValueError('The new version must exceed ' + current['tag_name'])
            print('Ready: restricted SSH key + HTTPS; current ' + current['tag_name'])
            return 0
        if args.probe:
            with tempfile.TemporaryDirectory(prefix='cview-release-probe-') as tmp:
                probe = Path(tmp) / 'probe.bin'
                probe.write_bytes(os.urandom(1024 * 1024))
                info = describe(probe)
                result = rpc({'action': 'probe', **info}, probe)
                if result['probe'] != info or not result['cleaned']:
                    raise ValueError('Upload/readback/cleanup probe failed')
            verified = verify_https(download=True)
            print('Verified: 1 MiB SSH upload/readback/SHA-256/cleanup; HTTPS downloads ' + verified['tag_name'])
            return 0
        if not args.version or not args.notes_file:
            parser.error('--version and --notes-file are required for upload/publish')
        names = filenames(args.version)
        paths = {name: args.dist / name for name in names[:2]}
        paths['notes.txt'] = args.notes_file
        notes = args.notes_file.read_bytes().decode('utf-8')
        if not notes.strip() or len(notes.encode('utf-8')) > 65536:
            raise ValueError('Provide nonempty UTF-8 notes up to 64 KiB')
        # Verify the actual DMG; bypassing the build wrapper must not bypass distribution checks.
        dmg = str(paths[names[0]])
        subprocess.run(['codesign', '--verify', '--strict', dmg], check=True)
        if args.allow_unnotarized:
            if '공증 안 함' not in notes:
                raise ValueError('Release notes must disclose: 공증 안 함')
        else:
            subprocess.run(['xcrun', 'stapler', 'validate', dmg], check=True)
            subprocess.run(['spctl', '-a', '-vv', '-t', 'install', dmg], check=True)
        if 'SmartScreen' not in notes:
            raise ValueError('Release notes must disclose unsigned Windows installer / SmartScreen')
        expected = {name: describe(path) for name, path in paths.items()}
        if any(item['size'] <= 0 for item in expected.values()):
            raise ValueError('Empty release file')
        if not same_release(current, args.version, expected):
            if version_tuple(args.version) <= version_tuple(current['tag_name'].lstrip('v')):
                raise ValueError('Version must be newer than ' + current['tag_name'])
            if state['free_bytes'] < 3 * sum(item['size'] for item in expected.values()):
                raise ValueError('Insufficient space on the update server')
            for name, path in paths.items():
                print('Uploading ' + name, flush=True)
                received = rpc({'action': 'upload', 'version': args.version, 'name': name, **expected[name]}, path)
                if any(received[k] != expected[name][k] for k in ('size', 'sha256')):
                    raise ValueError('Server upload acknowledgment differs')
        result = rpc({'action': 'validate' if args.validate_only else 'publish',
                      'version': args.version, 'files': expected})
        if args.validate_only:
            print('Uploaded and validated; publish was not requested. Already published: ' + str(result['published']))
            return 0
        manifest = verify_https(args.version, expected, download=True)
        receipt = args.dist / f'CView_{args.version}_publish-receipt.json'
        receipt.write_text(json.dumps({'verified_at': datetime.now(timezone.utc).isoformat(),
                           'server_verified': result['server_verified'], 'client_download_verified': True,
                           'manifest': manifest}, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
        print('Published and verified: ' + manifest['html_url'])
        print('Receipt: ' + str(receipt))
        return 0
    except (OSError, ValueError, RuntimeError, KeyError, subprocess.SubprocessError) as error:
        print(f'Release automation failed: {error}', file=sys.stderr)
        print('Do not mark the release complete. Keep the version/files/notes and retry the same publish command.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())

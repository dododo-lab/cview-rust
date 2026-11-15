#!/usr/bin/env python3
"""Submit source to the Windows build server, or run its local macOS worker."""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
MODULES = REPO / 'tools' / 'build-server'
sys.path.insert(0, str(MODULES if MODULES.is_dir() else HERE))
import build_common as common

CONFIG = Path(os.environ.get('CVIEW_BUILD_CONFIG_DIR', '~/.config/cview-build')).expanduser()


def settings():
    value = common.read_json(CONFIG / 'server.json')
    target = value['ssh_target']
    if not isinstance(target, str) or target.startswith('-') or any(c.isspace() for c in target):
        raise ValueError('Invalid build SSH target')
    host = target.rsplit('@', 1)[-1]
    if any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.:-[]' for c in host):
        raise ValueError('Invalid build SSH host')
    for name in ('ssh_key', 'known_hosts', 'work_dir', 'target_dir', 'readobj'):
        value[name] = Path(value[name]).expanduser().resolve()
    if not value['ssh_key'].is_file() or not value['known_hosts'].is_file():
        raise ValueError('Missing build key or pinned known_hosts')
    if value['ssh_key'].stat().st_mode & 0o077:
        raise ValueError('Build private key must have mode 0600')
    if value.get('tailscale_nc'):
        value['tailscale_nc'] = Path(value['tailscale_nc']).expanduser().absolute()
        if not os.access(value['tailscale_nc'], os.X_OK):
            raise ValueError('Configured Tailscale CLI is not executable')
    return value


def ssh_command(config):
    command = ['ssh', '-F', os.devnull, '-T', '-i', str(config['ssh_key']),
            '-o', 'BatchMode=yes', '-o', 'IdentitiesOnly=yes', '-o', 'PreferredAuthentications=publickey',
            '-o', 'ControlMaster=no', '-o', 'ControlPath=none', '-o', 'StrictHostKeyChecking=yes',
            '-o', 'UserKnownHostsFile=' + str(config['known_hosts']), '-o', 'GlobalKnownHostsFile=' + os.devnull,
            '-o', 'HostKeyAlgorithms=ssh-ed25519', '-o', 'ConnectTimeout=10',
            '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3']
    if config.get('tailscale_nc'):
        command += ['-o', 'ProxyCommand=' + shlex.quote(str(config['tailscale_nc'])) + ' nc %h %p']
    return command + ['--', config['ssh_target'], common.PROTOCOL]


def rpc(config, request, upload=None, download=None, timeout=180):
    header = (json.dumps(request) + '\n').encode('ascii')
    if len(header) > 16384:
        raise ValueError('Request is too large')
    with tempfile.TemporaryFile() as input_stream, tempfile.TemporaryFile() as output:
        input_stream.write(header)
        if upload:
            with Path(upload).open('rb') as body:
                shutil.copyfileobj(body, input_stream)
        input_stream.seek(0)
        result = subprocess.run(ssh_command(config), stdin=input_stream, stdout=output,
                                stderr=subprocess.PIPE, timeout=timeout)
        output.seek(0)
        response = output.readline(65537)
        try:
            if len(response) > 65536 or not response.endswith(b'\n'):
                raise ValueError('Bad response header')
            response = json.loads(response)
        except ValueError:
            raise RuntimeError('Build SSH failed: ' + result.stderr.decode('utf-8', 'replace').strip()) from None
        if result.returncode or not response.get('ok'):
            raise RuntimeError(response.get('error') or result.stderr.decode('utf-8', 'replace').strip())
        if download:
            common.receive(output, download, response['file'])
        return response


def run(command, cwd, env, log):
    log.write(('\n$ ' + ' '.join(map(str, command)) + '\n').encode('utf-8'))
    log.flush()
    subprocess.run(list(map(str, command)), cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)


def check_mac(config):
    if sys.platform != 'darwin':
        raise ValueError('The macOS worker must run on a Mac')
    for command in ('cargo', 'rustup', 'node', '7zz', 'codesign', 'xcrun'):
        if not shutil.which(command):
            raise ValueError('Missing Mac tool: ' + command)
    if not config['readobj'].is_file():
        raise ValueError('Set readobj to an installed llvm-readobj path in server.json')
    if not config['target_dir'].parent.is_dir():
        raise ValueError('Mac target parent does not exist (is the data volume mounted?)')
    subprocess.run(['cargo', 'tauri', '--version'], check=True, capture_output=True)


def preflight(config, windows_release=False):
    if windows_release:
        if not shutil.which('7zz') or not os.access(config['readobj'], os.X_OK):
            raise ValueError('Windows installer audits require 7zz and an executable readobj in server.json')
    else:
        check_mac(config)
    result = rpc(config, {'action': 'preflight'})
    if result['protocol'] != common.PROTOCOL or result['server_name'].casefold() != config['server_name'].casefold():
        raise ValueError('Wrong build server or protocol')
    if windows_release and common.WINDOWS_RELEASE_FEATURE not in result.get('features', []):
        raise ValueError('Build gateway needs redeploy: missing ' + common.WINDOWS_RELEASE_FEATURE)
    return result


def download_windows(config, current, artifacts, log):
    files = current['windows']['files']
    names = {f'CView_{current["version"]}_{arch}-setup.exe' for arch in common.TARGETS}
    if current['windows']['state'] != 'succeeded' or set(files) != names:
        raise ValueError('Both Windows installers must succeed')
    for expected in files.values():
        common.metadata(expected)
    installer_script = REPO / 'scripts/windows-installer.py' if MODULES.is_dir() else HERE / 'windows-installer.py'
    for arch, (_, machine) in common.TARGETS.items():
        name = f'CView_{current["version"]}_{arch}-setup.exe'
        installer = artifacts / name
        rpc(config, {'action': 'artifact', 'job': current['job'], 'name': name}, download=installer)
        if common.describe(installer) != files[name]:
            raise ValueError('Downloaded Windows installer hash differs')
        run([sys.executable, installer_script, 'check', '--installer', installer, '--machine', hex(machine),
             '--readobj', config['readobj']], artifacts, os.environ.copy(), log)
    return files


def release_windows(config, source, version, output, job=None, timeout=14400):
    """Build only Windows; the caller owns macOS notarization and publishing."""
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version) or timeout <= 0:
        raise ValueError('Invalid release version or timeout')
    preflight(config, windows_release=True)
    build_env = common.build_environment({key: os.environ[key] for key in ('CVIEW_UPDATE_URL',) if key in os.environ})
    if job:
        current = rpc(config, {'action': 'status', 'job': common.checked_id(job)})
    else:
        with tempfile.TemporaryDirectory(prefix='cview-windows-release-') as tmp:
            archive = Path(tmp).resolve() / 'source.zip'
            if common.snapshot(source, archive) != version:
                raise ValueError('Release version differs from the source snapshot')
            expected_source = common.describe(archive)
            job = common.job_id()
            current = rpc(config, {'action': 'submit', 'job': job, 'source': expected_source,
                                  'mode': common.WINDOWS_RELEASE_MODE, 'build_env': build_env}, upload=archive)
        if current['source'] != expected_source:
            raise ValueError('Server accepted a different source snapshot')
    identity = {'job': job, 'version': version, 'mode': common.WINDOWS_RELEASE_MODE,
                'source': current['source'], 'build_env': build_env}
    common.metadata(identity['source'], common.MAX_SOURCE)

    def check_identity(result):
        if any(result.get(key) != value for key, value in identity.items()):
            raise ValueError('Windows job source, mode, version or environment differs from this release')

    check_identity(current)
    directory = common.safe(config['work_dir'] / job)
    directory.mkdir(parents=True, exist_ok=True)
    common.write_json(directory / 'release-job.json', identity)
    print('Windows release job: ' + job + ' (source ' + current['source']['sha256'] + ')', flush=True)
    deadline, previous = time.monotonic() + timeout, None
    try:
        while True:
            check_identity(current)
            state = current['windows']['state']
            if state != previous:
                print('Windows x64 + ARM64: ' + state, flush=True)
                previous = state
            if state == 'failed':
                raise RuntimeError('Windows build failed: ' + current['windows'].get('error', 'see windows.log'))
            if state == 'succeeded':
                break
            if time.monotonic() >= deadline:
                raise TimeoutError('Windows release build timed out; job preserved: ' + job)
            time.sleep(10)
            current = rpc(config, {'action': 'status', 'job': job})
        # Stage and audit BOTH files before replacing any release output. A
        # corrupt download or failed audit must never expose a partial success.
        with tempfile.TemporaryDirectory(prefix='verified-', dir=directory) as tmp:
            staging = Path(tmp)
            with (directory / 'audit.log').open('ab', buffering=0) as log:
                files = download_windows(config, current, staging, log)
            rpc(config, {'action': 'complete-windows', 'job': job, 'verified_windows': files})
            rpc(config, {'action': 'receipt', 'job': job}, download=staging / 'receipt.json')
            receipt = common.read_json(staging / 'receipt.json')
            check_identity(receipt)
            if (receipt.get('files') != files or receipt.get('windows_dependency_audit') != 'passed-on-mac'
                    or receipt.get('published') is not False):
                raise ValueError('Windows build receipt differs from verified artifacts')
            output = common.safe(Path(output).resolve())
            output.mkdir(parents=True, exist_ok=True)
            for name, expected in files.items():
                with (staging / name).open('rb') as stream:
                    # Export atomically, replacing only this version's local
                    # output after verification (server artifacts stay immutable).
                    with tempfile.NamedTemporaryFile(dir=output, delete=False) as dest:
                        temporary = Path(dest.name)
                        try:
                            shutil.copyfileobj(stream, dest)
                            dest.flush()
                            os.fsync(dest.fileno())
                        except BaseException:
                            temporary.unlink(missing_ok=True)
                            raise
                try:
                    if common.describe(temporary) != expected:
                        raise ValueError('Local Windows export hash differs')
                    os.replace(temporary, common.safe(output / name))
                finally:
                    temporary.unlink(missing_ok=True)
                print(expected['sha256'] + '  ' + str(output / name), flush=True)
            common.write_json(output / f'CView_{version}_windows-build.json', receipt)
            common.write_json(directory / 'receipt.json', receipt)
    finally:
        audit_log = directory / 'audit.log'
        if audit_log.exists():
            print(audit_log.read_text(encoding='utf-8', errors='replace')[-32768:], flush=True)
        try:
            tail = rpc(config, {'action': 'log', 'job': job})['text']
            (directory / 'windows.log').write_text(tail, encoding='utf-8')
            print(tail, flush=True)
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            print('Could not fetch Windows log: ' + str(error), file=sys.stderr)
    print('Windows release installers verified: ' + str(output / f'CView_{version}_windows-build.json'), flush=True)
    return receipt


def mac_image(bundle, version):
    # create-dmg can leave a temporary rw.<pid>.*.dmg alongside its result.
    # Select the requested version/architecture, rather than counting images.
    path = common.safe(Path(bundle) / 'dmg' / f'CView_{version}_aarch64.dmg')
    if not path.is_file():
        raise ValueError('Expected Mac DMG is missing: ' + str(path))
    return path


def build_mac(config, job, directory):
    archive = directory / 'source.zip'
    rpc(config, {'action': 'source', 'job': job['job']}, download=archive)
    if common.describe(archive) != job['source']:
        raise ValueError('Source snapshot differs from the submitted job')
    source = directory / 'source'
    if source.exists():
        # Only this isolated job workspace is disposable. Never touch the checkout.
        common.safe(source)
        shutil.rmtree(source)
    common.validate_archive(archive, source)
    env = dict(os.environ, CARGO_TARGET_DIR=str(config['target_dir']), CI='true')
    target = 'aarch64-apple-darwin'
    identity = common.read_json(source / 'crates/cview-app/tauri.conf.json')['bundle']['macOS']['signingIdentity']
    identities = subprocess.check_output(['security', 'find-identity', '-v', '-p', 'codesigning'], text=True)
    if not identity or identity not in identities:
        raise ValueError('Configured Developer ID identity is unavailable in the login keychain')
    artifacts = directory / 'artifacts'
    artifacts.mkdir(exist_ok=True)
    with (directory / 'macos.log').open('ab', buffering=0) as log:
        run(['cargo', 'fmt', '--all', '--check'], source, env, log)
        run(['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'], source, env, log)
        run(['cargo', 'test', '--workspace', '--locked'], source, env, log)
        for path in sorted((source / 'app-ui/js').glob('*.js')):
            run(['node', '--check', path], source, env, log)
        bundle = config['target_dir'] / target / 'release/bundle'
        for old in (bundle / 'dmg').glob('*.dmg'):
            common.safe(old).unlink()
        run(['cargo', 'tauri', 'build', '--ci', '--target', target, '--bundles', 'app,dmg', '--', '--locked'],
            source / 'crates/cview-app', env, log)
        app = bundle / 'macos/CView.app'
        info = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
        if info['CFBundleShortVersionString'] != job['version']:
            raise ValueError('Mac app version differs from source')
        run(['lipo', '-verify_arch', 'arm64', app / 'Contents/MacOS/cview-app'], source, env, log)
        run(['codesign', '--verify', '--deep', '--strict', app], source, env, log)
        image = mac_image(bundle, job['version'])
        # Build-only output. Formal releases still use release-macos.sh --dmg,
        # which increments versions, notarizes, and publishes all three installers.
        run(['codesign', '--force', '--timestamp', '--sign', identity, image], source, env, log)
        run(['codesign', '--verify', '--strict', image], source, env, log)
        destination = artifacts / f'CView_{job["version"]}_arm64.dmg'
        shutil.copyfile(image, destination)
    return destination


def process_job(config, job):
    job_name, token = job['job'], job['macos']['token']
    directory = common.safe(config['work_dir'] / job_name)
    directory.mkdir(parents=True, exist_ok=True)
    print('Building job ' + job_name + ' (source ' + job['source']['sha256'] + ')', flush=True)
    try:
        check_mac(config)
        if job['macos']['state'] != 'succeeded':
            dmg = build_mac(config, job, directory)
            rpc(config, {'action': 'upload-macos', 'job': job_name, 'token': token, 'file': common.describe(dmg)}, upload=dmg)
        deadline = time.monotonic() + 4 * 60 * 60
        while True:
            current = rpc(config, {'action': 'status', 'job': job_name})
            if current['windows']['state'] == 'failed':
                raise RuntimeError('Windows build failed: ' + current['windows'].get('error', 'see windows.log'))
            if current['windows']['state'] == 'succeeded':
                break
            if time.monotonic() > deadline:
                raise TimeoutError('Windows build did not finish within four hours')
            time.sleep(10)
        artifacts = directory / 'artifacts'
        artifacts.mkdir(exist_ok=True)
        with (directory / 'macos.log').open('ab', buffering=0) as log:
            download_windows(config, current, artifacts, log)
        rpc(config, {'action': 'complete', 'job': job_name, 'token': token, 'verified_windows': current['windows']['files']})
        rpc(config, {'action': 'receipt', 'job': job_name}, download=directory / 'receipt.json')
        print('Build complete: ' + str(directory / 'receipt.json'), flush=True)
    except Exception as error:
        try:
            rpc(config, {'action': 'fail-macos', 'job': job_name, 'token': token, 'error': str(error)[:4096]})
        except Exception as report_error:
            print('Could not report failure: ' + str(report_error), file=sys.stderr, flush=True)
        raise


def worker(config, once=False, requested=None):
    check_mac(config)
    config['work_dir'].mkdir(parents=True, exist_ok=True)
    # Launchd and manual invocations share one worker/cache per Mac.
    with common.locked(config['work_dir'] / 'worker.lock'):
        active = config['work_dir'] / 'active.json'
        if active.exists():
            previous = rpc(config, {'action': 'status', 'job': common.read_json(active)['job']})
            if previous['state'] == 'pending':
                if previous['macos'].get('worker') != config['worker']:
                    raise ValueError('Saved active job belongs to another Mac worker')
                process_job(config, previous)
                active.unlink()
                if once or requested:
                    return
            else:
                active.unlink()
        while True:
            request = {'action': 'claim', 'worker': config['worker']}
            if requested:
                request['job'] = common.checked_id(requested)
            response = rpc(config, request)
            if response['job']:
                common.write_json(active, {'job': response['job']['job']})
                process_job(config, response['job'])
                active.unlink()
            if once or requested:
                return
            if not response['job']:
                time.sleep(15)


def submit(config, source):
    # Snapshot first: no later checkout edits or deletions can alter this job.
    with tempfile.TemporaryDirectory(prefix='cview-build-') as tmp:
        archive = Path(tmp).resolve() / 'source.zip'
        common.snapshot(source, archive)
        result = rpc(config, {'action': 'submit', 'job': common.job_id(), 'source': common.describe(archive)}, upload=archive)
    print('Submitted: ' + result['job'], flush=True)
    return result['job']


def install_worker(config):
    check_mac(config)
    destination = CONFIG / 'bin'
    destination.mkdir(parents=True, exist_ok=True)
    copies = {Path(__file__): destination / 'build-server.py',
              MODULES / 'build_common.py': destination / 'build_common.py',
              REPO / 'scripts/windows-installer.py': destination / 'windows-installer.py'}
    for source, target in copies.items():
        if source.resolve() != target.resolve():
            shutil.copyfile(source, target)
    logs = CONFIG / 'logs'
    logs.mkdir(exist_ok=True)
    label = 'app.cview.build-worker'
    agent = Path.home() / 'Library/LaunchAgents' / (label + '.plist')
    agent.parent.mkdir(parents=True, exist_ok=True)
    payload = {'Label': label, 'ProgramArguments': ['/usr/bin/caffeinate', '-i', sys.executable,
                str(destination / 'build-server.py'), 'worker', '--once'], 'RunAtLoad': True, 'StartInterval': 15,
               'ProcessType': 'Standard', 'StandardOutPath': str(logs / 'worker.log'),
               'StandardErrorPath': str(logs / 'worker-error.log'), 'EnvironmentVariables':
               {'PATH': str(Path.home() / '.cargo/bin') + ':/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin',
                'CVIEW_BUILD_CONFIG_DIR': str(CONFIG), 'LANG': 'en_US.UTF-8'}}
    agent.write_bytes(plistlib.dumps(payload))
    domain = 'gui/' + str(os.getuid())
    subprocess.run(['launchctl', 'bootout', domain + '/' + label], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(['launchctl', 'bootstrap', domain, str(agent)], check=True)
    print('Installed Mac worker: ' + str(agent))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('preflight')
    sub.add_parser('windows-preflight')
    sub.add_parser('install-worker')
    sub.add_parser('setup-log')
    submit_parser = sub.add_parser('submit')
    submit_parser.add_argument('--source', type=Path, default=Path.cwd())
    submit_parser.add_argument('--wait', action='store_true')
    worker_parser = sub.add_parser('worker')
    worker_parser.add_argument('--once', action='store_true')
    worker_parser.add_argument('--job')
    status_parser = sub.add_parser('status')
    status_parser.add_argument('job', nargs='?')
    log_parser = sub.add_parser('log')
    log_parser.add_argument('job')
    release_parser = sub.add_parser('release-windows', help='Build/audit both Windows installers for release-macos.sh')
    release_parser.add_argument('--source', type=Path, default=Path.cwd())
    release_parser.add_argument('--version', required=True)
    release_parser.add_argument('--output', type=Path, default=Path('dist'))
    release_parser.add_argument('--job', help='Resume downloading/auditing an existing Windows release job')
    release_parser.add_argument('--timeout', type=int, default=14400)
    args = parser.parse_args()
    config = settings()
    if args.command in ('preflight', 'windows-preflight'):
        print(json.dumps(preflight(config, args.command == 'windows-preflight'), indent=2))
    elif args.command == 'release-windows':
        release_windows(config, args.source.resolve(), args.version, args.output, args.job, args.timeout)
    elif args.command == 'install-worker':
        install_worker(config)
    elif args.command == 'worker':
        try:
            worker(config, args.once, args.job)
        except BlockingIOError:
            # A foreground worker may already be building. Launchd's periodic
            # probe must leave that build alone without reporting a failure.
            if not args.once:
                raise
    elif args.command == 'setup-log':
        print(rpc(config, {'action': 'setup-log'})['text'])
    elif args.command == 'submit':
        job = submit(config, args.source.resolve())
        if args.wait:
            while True:
                result = rpc(config, {'action': 'status', 'job': job})
                print('Windows: ' + result['windows']['state'] + ' / macOS: ' + result['macos']['state'], flush=True)
                if result['state'] != 'pending':
                    print(json.dumps(result, indent=2))
                    return 0 if result['state'] == 'succeeded' else 1
                time.sleep(10)
    elif args.command == 'status':
        print(json.dumps(rpc(config, {'action': 'status', 'job': common.checked_id(args.job)} if args.job else {'action': 'list'}), indent=2))
    elif args.command == 'log':
        print(rpc(config, {'action': 'log', 'job': common.checked_id(args.job)})['text'])
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print('Build failed: ' + str(error), file=sys.stderr)
        sys.exit(1)

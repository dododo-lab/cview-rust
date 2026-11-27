"""Detached Windows-target build process for the installed queue tools."""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import time

import build_common as common


def pe_machine(path):
    with Path(path).open('rb') as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b'MZ':
            raise ValueError('Not a PE file: ' + str(path))
        stream.seek(struct.unpack_from('<I', header, 0x3C)[0])
        pe = stream.read(6)
    if len(pe) != 6 or pe[:4] != b'PE\0\0':
        raise ValueError('Invalid PE header: ' + str(path))
    return struct.unpack_from('<H', pe, 4)[0]


def native_logged(log_path, command):
    """Capture native stdout/stderr without PowerShell's detached console host.

    PowerShell 5 may allocate an invisible console for native programs in a
    detached SSH worker and send their output there, ignoring the inherited log
    handle. This helper gives each native process an explicit output handle and
    CREATE_NO_WINDOW, and preserves its real exit code.
    """
    log_path = common.safe(Path(log_path))
    with log_path.open('ab', buffering=0) as output:
        output.write(('\n$ ' + ' '.join(command) + '   [' + time.strftime('%H:%M:%S') + ']\n').encode('utf-8'))
        started = time.monotonic()
        try:
            result = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                                    creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
            output.write(('  (exit ' + str(result.returncode) + ', ' + elapsed(started) + ')\n').encode('utf-8'))
            return result.returncode
        except OSError as error:
            output.write(('Cannot start native command: ' + str(error) + '\n').encode('utf-8'))
            return 1


def elapsed(started):
    seconds = int(time.monotonic() - started)
    return f'{seconds // 60}m{seconds % 60:02d}s'


def kill_tree(process):
    """Stop a lane and its cargo/rustc children (TerminateProcess alone leaves them holding the target lock)."""
    if process.poll() is not None:
        return
    if os.name == 'nt':
        subprocess.run(['taskkill', '/T', '/F', '/PID', str(process.pid)], stdin=subprocess.DEVNULL,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    else:
        process.kill()
    process.wait()


def run_lanes(lanes, log_path, start, poll=0.5):
    """Run build lanes concurrently and merge their logs in lane order.

    `start(name, lane_log)` returns a Popen-like process. The first failing lane
    stops the others so a broken check does not wait for two release builds.
    Returns the names of lanes that failed or were stopped.
    """
    log_path = common.safe(Path(log_path))
    running, results = {}, {}
    for name in lanes:
        lane_log = log_path.with_name(log_path.stem + '-' + name + '.log')
        running[name] = (start(name, lane_log), lane_log, time.monotonic())
    stopped = set()
    while len(results) < len(running):
        for name, (process, _, started) in running.items():
            if name in results or process.poll() is None:
                continue
            results[name] = (process.returncode, elapsed(started))
            if process.returncode != 0:
                for other, (other_process, _, other_started) in running.items():
                    if other not in results:
                        kill_tree(other_process)
                        stopped.add(other)
                        results[other] = (other_process.returncode, elapsed(other_started))
        if len(results) < len(running):
            time.sleep(poll)
    failed = []
    with log_path.open('ab', buffering=0) as output:
        for name, (_, lane_log, _) in running.items():
            code, took = results[name]
            state = 'stopped' if name in stopped else ('ok' if code == 0 else f'failed (exit {code})')
            if state != 'ok':
                failed.append(name)
            output.write(f'\n===== lane {name}: {state}, {took} =====\n'.encode('utf-8'))
            if lane_log.exists():
                output.write(lane_log.read_bytes())
        output.write(('\nLanes: ' + ', '.join(f'{n} {results[n][1]}' for n in running) + '\n').encode('utf-8'))
    return failed


def run(root, job):
    directory = common.job_path(root, job)
    try:
        with common.locked(root / 'windows.lock', blocking=True):
            current = common.status(root, job)
            if current['windows']['state'] == 'succeeded':
                return 0
            common.write_json(directory / 'windows.json', {'state': 'running', 'started': common.now(), 'pid': os.getpid()})
            source = directory / 'windows-source'
            if source.exists():
                raise ValueError('Existing build workspace; submit a new job after an interrupted build')
            if common.describe(directory / 'source.zip') != current['source']:
                raise ValueError('Source snapshot hash differs')
            common.validate_archive(directory / 'source.zip', source)
            env = os.environ.copy()
            env.pop('CVIEW_UPDATE_URL', None)
            env.update(common.build_environment(current.get('build_env', {})))

            def start(lane, lane_log):
                target_dir = root / common.LANE_TARGET_DIRS[lane]
                artifacts = directory / 'artifacts'
                if os.name == 'nt':
                    return subprocess.Popen([
                        'powershell.exe', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                        '-File', str(Path(__file__).with_name('Build-Windows.ps1')), '-Root', str(root),
                        '-SourcePath', str(source), '-OutputPath', str(artifacts),
                        '-LogPath', str(lane_log), '-Lane', lane, '-TargetDir', str(target_dir),
                    ], stdin=subprocess.DEVNULL, env=env)
                with lane_log.open('ab') as log:
                    return subprocess.Popen([
                        '/bin/bash', str(Path(__file__).with_name('Build-Windows.sh')), str(root),
                        str(source), lane, str(target_dir), str(artifacts), current['version'], current['mode'],
                    ], stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, env=env)

            lanes = [name for name, _ in common.LANES]
            groups = [lanes] if os.name == 'nt' else common.LINUX_LANE_GROUPS
            for group in groups:
                failed = run_lanes(group, directory / 'windows.log', start)
                if failed:
                    break
            if failed:
                raise ValueError('Windows build lane failed: ' + ', '.join(failed))
            files = {}
            for arch, (target, machine) in common.TARGETS.items():
                binary = root / common.LANE_TARGET_DIRS[arch] / target / 'release' / 'cview-app.exe'
                if pe_machine(binary) != machine:
                    raise ValueError('Wrong PE architecture: ' + arch)
                name = f'CView_{current["version"]}_{arch}-setup.exe'
                files[name] = common.describe(directory / 'artifacts' / name)
            result = {'state': 'succeeded', 'finished': common.now(), 'files': files}
            if os.name != 'nt' and current['mode'] == common.WINDOWS_RELEASE_MODE:
                result['tests'] = common.metadata(common.describe(directory / 'artifacts' / common.WINDOWS_TEST_ARCHIVE))
            common.write_json(directory / 'windows.json', result)
        return 0
    except Exception as error:
        common.write_json(directory / 'windows.json', {'state': 'failed', 'finished': common.now(), 'error': str(error)})
        with (directory / 'windows.log').open('ab', buffering=0) as output:
            output.write(('\nBuild failed: ' + str(error) + '\n').encode('utf-8'))
        return 1


def main():
    if len(sys.argv) >= 5 and sys.argv[1] == '--native-log' and sys.argv[3] == '--':
        return native_logged(sys.argv[2], sys.argv[4:])
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--job', required=True)
    args = parser.parse_args()
    return run(common.safe(args.root), common.checked_id(args.job))


if __name__ == '__main__':
    sys.exit(main())

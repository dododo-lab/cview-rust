"""Detached Windows build process; called only by the installed queue tools."""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import sys

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
        output.write(('\n$ ' + ' '.join(command) + '\n').encode('utf-8'))
        try:
            result = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                                    creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
            return result.returncode
        except OSError as error:
            output.write(('Cannot start native command: ' + str(error) + '\n').encode('utf-8'))
            return 1


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
            subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
                            '-File', str(Path(__file__).with_name('Build-Windows.ps1')), '-Root', str(root),
                            '-SourcePath', str(source), '-OutputPath', str(directory / 'artifacts'),
                            '-LogPath', str(directory / 'windows.log')], check=True, env=env)
            files = {}
            for arch, (target, machine) in common.TARGETS.items():
                binary = root / 'target' / target / 'release' / 'cview-app.exe'
                if pe_machine(binary) != machine:
                    raise ValueError('Wrong PE architecture: ' + arch)
                name = f'CView_{current["version"]}_{arch}-setup.exe'
                files[name] = common.describe(directory / 'artifacts' / name)
            common.write_json(directory / 'windows.json',
                              {'state': 'succeeded', 'finished': common.now(), 'files': files})
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

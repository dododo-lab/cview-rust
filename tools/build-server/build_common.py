"""Source snapshots and job storage shared by the Windows coordinator and Mac worker.

This is a build queue, not a release publisher. Uploaded source is executable code;
only trusted maintainers may submit it.
"""
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid
from urllib.parse import urlsplit
import zipfile

PROTOCOL = 'cview-build-v1'
WINDOWS_RELEASE_FEATURE = 'windows-release-v1'
WINDOWS_CROSS_BUILD_FEATURE = 'windows-cross-build-v1'
WINDOWS_RUNTIME_FEATURE = 'windows-release-parallels-v1'
WINDOWS_TEST_ARCHIVE = 'x64-tests.zip'
WINDOWS_RELEASE_MODE = 'windows-release'
MAX_SOURCE = 128 * 1024**2
MAX_EXPANDED = 256 * 1024**2
MAX_ARTIFACT = 2 * 1024**3
ROOT_FILES = ('Cargo.toml', 'Cargo.lock', 'tools/windows-update-server/tests/signed_feed_vector.json')
TARGETS = {'x64': ('x86_64-pc-windows-msvc', 0x8664),
           'arm64': ('aarch64-pc-windows-msvc', 0xAA64)}
# Windows 빌드 갈래 — 검사(fmt·clippy·test x64)와 x64·ARM64 설치 파일을 동시에 만든다. 갈래마다 CARGO_TARGET_DIR 을
# 따로 써서 cargo 빌드 폴더 잠금으로 서로 기다리지 않는다(x64 는 예전 `target` 캐시를 그대로 쓴다).
LANES = (('check', 'target-check'), ('x64', 'target'), ('arm64', 'target-arm64'))
LANE_TARGET_DIRS = dict(LANES)
# Ubuntu 서버(4코어·메모리 7GB)에서 함께 돌릴 갈래 묶음. 릴리스 빌드(fat LTO, codegen-units=1)는 끝 약 1분에
# rustc 하나가 4.8GB까지 올라가 x64·ARM64 를 겹치면 메모리를 넘는다(v0.2.39 arm64 실측: 앞 대부분 1.2~1.7GB·CPU 25%).
# 검사(clippy·테스트 컴파일, 약 4분)는 LTO 를 하지 않아 x64 와 겹친다 — x64 의 4.8GB 구간은 시작 약 4.5분 뒤라 검사가
# 대개 먼저 끝나고, 겹쳐도 스왑(4GB)이 받친다. ARM64 는 그 뒤 혼자 돈다.
LINUX_LANE_GROUPS = (('check', 'x64'), ('arm64',))


def now():
    return datetime.now(timezone.utc).isoformat()


def job_id():
    return datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ-') + uuid.uuid4().hex[:12]


def checked_id(value):
    if not isinstance(value, str) or not re.fullmatch(r'\d{8}T\d{6}Z-[a-f0-9]{12}', value):
        raise ValueError('Invalid job ID')
    return value


def safe(path):
    path = Path(path)
    for part in (path, *path.parents):
        if part.is_symlink() or (part.exists() and getattr(part.lstat(), 'st_file_attributes', 0) & 0x400):
            raise ValueError('Links and reparse points are not allowed: ' + str(part))
    return path


def describe(path):
    safe(path)
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024**2), b''):
            digest.update(chunk)
    return {'size': Path(path).stat().st_size, 'sha256': digest.hexdigest()}


def metadata(value, limit=MAX_ARTIFACT):
    if (not isinstance(value, dict) or set(value) != {'size', 'sha256'}
            or type(value['size']) is not int or not 0 < value['size'] <= limit
            or not isinstance(value['sha256'], str) or not re.fullmatch('[a-f0-9]{64}', value['sha256'])):
        raise ValueError('Invalid file size or SHA-256')
    return value


def validate_windows_runtime(report, files, tests, version):
    if (not isinstance(report, dict) or report.get('host_architecture') != 'Arm64'
            or type(report.get('session_id')) is not int or report['session_id'] <= 0
            or 'error' not in report or report['error'] is not None):
        raise ValueError('An interactive Windows ARM64 validation report is required')
    tested = report.get('installers')
    if not isinstance(tested, list) or len(tested) != len(TARGETS):
        raise ValueError('Both Windows installers must be tested')
    installers = []
    for arch in TARGETS:
        name = f'CView_{version}_{arch}-setup.exe'
        installer = {'architecture': arch, 'installer_sha256': files[name]['sha256'],
                     'installed': True, 'webview2_started': True}
        if not any(isinstance(item, dict) and item.get('installed') is True
                   and item.get('webview2_started') is True and item == installer for item in tested):
            raise ValueError('Windows did not confirm installation and startup for ' + name)
        installers.append(installer)
    test_result = report.get('x64_tests')
    if (not isinstance(test_result, dict) or test_result.get('sha256') != metadata(tests)['sha256']
            or any(type(test_result.get(key)) is not int for key in ('suites', 'passed', 'failed', 'ignored'))
            or min(test_result['suites'], test_result['passed']) <= 0
            or test_result['failed'] != 0 or test_result['ignored'] < 0):
        raise ValueError('The exact Windows x64 test archive must pass in Parallels')
    return {'host_architecture': 'Arm64', 'session_id': report['session_id'], 'error': None,
            'installers': installers, 'x64_tests': {key: test_result[key]
                                                    for key in ('sha256', 'suites', 'passed', 'failed', 'ignored')}}


def write_json(path, value):
    safe(path)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as stream:
        tmp = Path(stream.name)
        try:
            stream.write((json.dumps(value, ensure_ascii=True, indent=2) + '\n').encode('utf-8'))
            stream.flush()
            os.fsync(stream.fileno())
        except BaseException:
            tmp.unlink(missing_ok=True)
            raise
    try:
        os.replace(tmp, path)
    finally:
        tmp.unlink(missing_ok=True)


def read_json(path):
    return json.loads(safe(Path(path)).read_text(encoding='utf-8'))


@contextmanager
def locked(path, blocking=False):
    safe(path)
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with Path(path).open('a+b') as stream:
        if stream.tell() == 0:
            stream.write(b'0')
            stream.flush()
        stream.seek(0)
        if os.name == 'nt':
            import msvcrt
            # LK_LOCK retries only ten times; use explicit retries for long builds.
            import time
            while True:
                try:
                    msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    if not blocking:
                        raise
                    time.sleep(1)
        else:
            import fcntl
            fcntl.flock(stream, fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
        yield


def source_name(name):
    """Portable file paths: reject Windows aliases, ADS, links and local secrets."""
    if not isinstance(name, str) or '\\' in name or ':' in name or '\x00' in name:
        raise ValueError('Invalid source path')
    path = PurePosixPath(name)
    if path.is_absolute() or str(path) != name or any(p in ('', '.', '..') for p in path.parts):
        raise ValueError('Invalid source path')
    if name not in ROOT_FILES and path.parts[0] not in ('crates', 'app-ui'):
        raise ValueError('Only the declared Cargo, test fixture, crates/ and app-ui/ files are build inputs')
    for part in path.parts:
        if (part.endswith((' ', '.')) or any(ord(c) < 32 or c in '<>"|?*' for c in part)
                or re.fullmatch(r'(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?', part)
                or part.lower() in ('target', 'node_modules', '.git', '.cargo', 'gen')
                or part.lower().startswith('.env')
                or part.lower().endswith(('.pem', '.key', '.p12', '.pfx'))):
            raise ValueError('Unsafe or local-only source path: ' + name)
    return name


def source_files(root):
    root = Path(root).resolve()
    names = list(ROOT_FILES)
    for folder in ('crates', 'app-ui'):
        for base, directories, files in os.walk(root / folder, followlinks=False):
            directories[:] = sorted(d for d in directories if d not in ('target', 'gen', 'node_modules', '.git', '.cargo'))
            for directory in directories:
                safe(Path(base) / directory)
            for filename in sorted(files):
                path = Path(base) / filename
                name = path.relative_to(root).as_posix()
                # Do not silently upload credentials placed in a source directory.
                source_name(name)
                safe(path)
                if path.is_file():
                    names.append(name)
    return names


def snapshot(root, output):
    total = 0
    with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as archive:
        for name in source_files(root):
            path = safe(Path(root) / name)
            total += path.stat().st_size
            if total > MAX_EXPANDED:
                raise ValueError('Source exceeds expanded size limit')
            archive.write(path, name)
    if Path(output).stat().st_size > MAX_SOURCE:
        raise ValueError('Source archive exceeds upload limit')
    return validate_archive(output)


def validate_archive(path, destination=None):
    names, folded, total = set(), set(), 0
    with zipfile.ZipFile(path) as archive:
        if len(archive.infolist()) > 20000:
            raise ValueError('Too many source files')
        for item in archive.infolist():
            name = source_name(item.filename)
            mode = item.external_attr >> 16
            if item.is_dir() or stat.S_ISLNK(mode) or (stat.S_IFMT(mode) not in (0, stat.S_IFREG)):
                raise ValueError('Source must contain regular files only')
            if name.casefold() in folded:
                raise ValueError('Duplicate/case-colliding source paths')
            names.add(name)
            folded.add(name.casefold())
            total += item.file_size
            if total > MAX_EXPANDED:
                raise ValueError('Source exceeds expanded size limit')
        required = {*ROOT_FILES, 'crates/cview-app/Cargo.toml',
                    'crates/cview-app/tauri.conf.json', 'app-ui/index.html'}
        if not required <= names:
            raise ValueError('Incomplete source snapshot: ' + ', '.join(sorted(required - names)))
        config = json.loads(archive.read('crates/cview-app/tauri.conf.json'))
        version = config['version']
        if not isinstance(version, str) or not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version):
            raise ValueError('Invalid source version')
        cargo = archive.read('Cargo.toml').decode('utf-8')
        match = re.search(r'\[workspace\.package\]([^\[]*)', cargo)
        if not match or not re.search(r'^version\s*=\s*"' + re.escape(version) + r'"\s*$', match[1], re.M):
            raise ValueError('Cargo and Tauri versions differ')
        if destination is not None:
            destination = safe(Path(destination))
            destination.mkdir(parents=True, exist_ok=False)
            for name in sorted(names):
                target = safe(destination / name)
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(name) as src, target.open('xb') as dst:
                    shutil.copyfileobj(src, dst)
    return version


def receive(stream, destination, expected, limit=MAX_ARTIFACT):
    metadata(expected, limit)
    destination = safe(Path(destination))
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as output:
        tmp = Path(output.name)
        try:
            remaining = expected['size']
            while remaining:
                chunk = stream.read(min(remaining, 1024**2))
                if not chunk:
                    raise ValueError('Truncated upload')
                output.write(chunk)
                remaining -= len(chunk)
            output.flush()
            os.fsync(output.fileno())
        except BaseException:
            tmp.unlink(missing_ok=True)
            raise
    try:
        if describe(tmp) != expected:
            raise ValueError('Upload SHA-256 mismatch')
        if destination.exists():
            if describe(destination) != expected:
                raise ValueError('Existing file differs; never overwrite a job artifact')
        else:
            os.replace(tmp, destination)
    finally:
        tmp.unlink(missing_ok=True)
    return expected


def job_path(root, job):
    return safe(Path(root) / 'jobs' / checked_id(job))


def status(root, job):
    directory = job_path(root, job)
    result = read_json(directory / 'job.json')
    for platform in ('windows', 'macos'):
        path = directory / (platform + '.json')
        fallback = 'external' if platform == 'macos' and result['mode'] == WINDOWS_RELEASE_MODE else 'queued'
        result[platform] = read_json(path) if path.exists() else {'state': fallback}
    receipt = directory / 'receipt.json'
    result['state'] = ('succeeded' if receipt.exists() else
                       'failed' if any(result[p]['state'] == 'failed' for p in ('windows', 'macos')) else 'pending')
    return result


def build_environment(value):
    # Only this public compile-time setting may cross the SSH boundary. Never
    # forward the release process's signing credentials or arbitrary variables.
    if not isinstance(value, dict) or set(value) - {'CVIEW_UPDATE_URL'}:
        raise ValueError('Unsupported build environment')
    if 'CVIEW_UPDATE_URL' in value:
        url = value['CVIEW_UPDATE_URL']
        if not isinstance(url, str) or len(url) > 2048 or any(ord(c) < 32 for c in url):
            raise ValueError('Invalid CVIEW_UPDATE_URL')
        if url:
            parsed = urlsplit(url)
            if parsed.scheme != 'https' or not parsed.hostname or parsed.username or parsed.password:
                raise ValueError('CVIEW_UPDATE_URL must be a public HTTPS URL')
    return dict(value)


def submit(root, job, stream, expected, start=True, mode='build-only', build_env=None):
    root = safe(Path(root))
    metadata(expected, MAX_SOURCE)
    if mode not in ('build-only', WINDOWS_RELEASE_MODE):
        raise ValueError('Unsupported build mode')
    build_env = build_environment({} if build_env is None else build_env)
    directory = job_path(root, job)
    with locked(root / 'queue.lock', blocking=True):
        if (directory / 'job.json').exists():
            saved = read_json(directory / 'job.json')
            if saved['source'] != expected or saved['mode'] != mode or saved.get('build_env', {}) != build_env:
                raise ValueError('Job already exists with different source, mode or environment')
            return status(root, job)
        directory.mkdir(parents=True, exist_ok=True)
        source = directory / 'source.zip'
        receive(stream, source, expected, MAX_SOURCE)
        version = validate_archive(source)
        record = {'job': job, 'version': version, 'source': expected, 'created': now(),
                  'mode': mode, 'build_env': build_env}
        if start and os.name != 'nt' and mode == WINDOWS_RELEASE_MODE:
            record['windows_backend'] = 'xwin'
        if mode == 'build-only':
            record['macos_notarized'] = False
        write_json(directory / 'job.json', record)
        if start:
            start_windows(root, job)
    return status(root, job)


def start_windows(root, job):
    directory = job_path(root, job)
    if os.name == 'nt':
        # Windows OpenSSH kills its session job on disconnect.
        detach = {'creationflags': (subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
                                     | subprocess.CREATE_BREAKAWAY_FROM_JOB)}
    else:
        detach = {'start_new_session': True}
    try:
        with (directory / 'windows.log').open('ab') as output:
            process = subprocess.Popen([sys.executable, str(Path(__file__).with_name('build_runner.py')),
                                        '--root', str(root), '--job', job], stdin=subprocess.DEVNULL,
                                       stdout=output, stderr=subprocess.STDOUT, close_fds=True, **detach)
    except OSError as error:
        write_json(directory / 'windows.json', {'state': 'failed', 'finished': now(), 'error': str(error)})
        raise
    # The runner owns windows.json; avoid overwriting its result after a fast failure.
    write_json(directory / 'windows-process.json', {'pid': process.pid, 'started': now()})

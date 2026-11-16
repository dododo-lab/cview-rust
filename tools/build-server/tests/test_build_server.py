import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import zipfile

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
import build_common as common
import build_gateway as gateway
import build_runner as runner

spec = importlib.util.spec_from_file_location('build_client', TOOLS.parents[1] / 'scripts/build-server.py')
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)


def source_zip(extra=None, cargo_version='1.2.3'):
    files = {'Cargo.toml': '[workspace.package]\nversion = "' + cargo_version + '"\n',
             'Cargo.lock': '# isolated fixture, never compiled or published\n',
             'crates/cview-app/Cargo.toml': '[package]\nname="test-only"\n',
             'crates/cview-app/tauri.conf.json': json.dumps({'version': '1.2.3'}),
             'tools/windows-update-server/tests/signed_feed_vector.json': '{"isolated_fixture":true}',
             'app-ui/index.html': '<p>isolated test fixture</p>'}
    files.update(extra or {})
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as archive:
        for name, value in files.items():
            archive.writestr(name, value)
    return output.getvalue()


def info(data):
    return {'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.job = common.job_id()
        self.worker = 'a' * 32

    def submit(self, data=None, **options):
        data = data or source_zip()
        return common.submit(self.root, self.job, io.BytesIO(data), info(data), start=False, **options)

    def dispatch(self, action, **args):
        return gateway.dispatch(self.root, {'action': action, 'job': self.job, **args}, io.BytesIO())[0]

    def claim(self):
        return self.dispatch('claim', worker=self.worker)['job']['macos']['token']

    def windows_success(self):
        directory = common.job_path(self.root, self.job)
        files = {}
        for arch in common.TARGETS:
            name = f'CView_1.2.3_{arch}-setup.exe'
            path = directory / 'artifacts' / name
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(('isolated-' + arch).encode())
            files[name] = common.describe(path)
        common.write_json(directory / 'windows.json', {'state': 'succeeded', 'files': files})
        return files

    def upload_mac(self, token):
        data = b'isolated test DMG payload; not a real installer'
        return gateway.dispatch(self.root, {'action': 'upload-macos', 'job': self.job, 'token': token,
                                            'file': info(data)}, io.BytesIO(data))[0]

    def test_source_archive_roundtrip_keeps_changed_files(self):
        archive = self.root / 'input.zip'
        archive.write_bytes(source_zip({'crates/changed.rs': 'changed\n'}))
        destination = self.root / 'source'
        self.assertEqual(common.validate_archive(archive, destination), '1.2.3')
        self.assertEqual((destination / 'crates/changed.rs').read_text(), 'changed\n')
        result = self.root / 'output.zip'
        self.assertEqual(common.snapshot(destination, result), '1.2.3')
        with zipfile.ZipFile(result) as zipped:
            self.assertEqual(zipped.read('crates/changed.rs'), b'changed\n')

    def test_deleted_files_do_not_leak_between_jobs(self):
        for name, extra in [('old', {'crates/deleted.rs': 'old'}), ('new', {})]:
            archive = self.root / (name + '.zip')
            archive.write_bytes(source_zip(extra))
            common.validate_archive(archive, self.root / name)
        self.assertFalse((self.root / 'new/crates/deleted.rs').exists())

    def test_traversal_ads_reserved_and_local_paths_rejected(self):
        for name in ('../escape', '/absolute', 'crates/../escape', 'crates//foo', 'crates/a:stream',
                     'crates/a\\b', 'crates/CON.rs', 'crates/file. ', '.ssh/id_ed25519',
                     'crates/.env', 'crates/key.pem', 'crates/target/old.exe', '.cargo/config.toml'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                common.source_name(name)

    def test_case_colliding_zip_names_rejected(self):
        archive = self.root / 'source.zip'
        archive.write_bytes(source_zip({'crates/a.rs': 'a', 'crates/A.rs': 'b'}))
        with self.assertRaisesRegex(ValueError, 'case-colliding'):
            common.validate_archive(archive)

    def test_zip_symlinks_rejected(self):
        archive = self.root / 'source.zip'
        archive.write_bytes(source_zip())
        with zipfile.ZipFile(archive, 'a') as output:
            item = zipfile.ZipInfo('crates/link')
            item.external_attr = (stat.S_IFLNK | 0o777) << 16
            output.writestr(item, '/outside')
        with self.assertRaisesRegex(ValueError, 'regular files'):
            common.validate_archive(archive)

    def test_local_symlinks_never_uploaded(self):
        source = self.root / 'source'
        archive = self.root / 'source.zip'
        archive.write_bytes(source_zip())
        common.validate_archive(archive, source)
        (source / 'crates/link.rs').symlink_to(self.root / 'outside')
        with self.assertRaisesRegex(ValueError, 'Links'):
            common.snapshot(source, self.root / 'output.zip')

    def test_expanded_size_limited_before_extraction(self):
        archive = self.root / 'source.zip'
        archive.write_bytes(source_zip())
        with patch.object(common, 'MAX_EXPANDED', 20), self.assertRaisesRegex(ValueError, 'expanded'):
            common.validate_archive(archive, self.root / 'source')
        self.assertFalse((self.root / 'source').exists())

    def test_incomplete_or_mismatched_sources_rejected(self):
        archive = self.root / 'source.zip'
        archive.write_bytes(source_zip(cargo_version='1.2.2'))
        with self.assertRaisesRegex(ValueError, 'versions differ'):
            common.validate_archive(archive)
        with zipfile.ZipFile(archive, 'w') as output:
            output.writestr('Cargo.toml', 'incomplete')
        with self.assertRaisesRegex(ValueError, 'Incomplete'):
            common.validate_archive(archive)

    def test_truncated_and_hash_mismatched_uploads_leave_no_artifact(self):
        destination = self.root / 'artifact'
        for payload in (b'abc', b'x' * 100):
            with self.assertRaises(ValueError):
                common.receive(io.BytesIO(payload), destination, info(b'original' * 10))
        self.assertFalse(destination.exists())
        self.assertEqual(list(self.root.iterdir()), [])

    def test_immutable_upload_allows_identical_retry(self):
        destination = self.root / 'artifact'
        common.receive(io.BytesIO(b'abc'), destination, info(b'abc'))
        common.receive(io.BytesIO(b'abc'), destination, info(b'abc'))
        with self.assertRaisesRegex(ValueError, 'never overwrite'):
            common.receive(io.BytesIO(b'changed'), destination, info(b'changed'))
        self.assertEqual(destination.read_bytes(), b'abc')

    def test_malformed_ids_and_metadata_rejected(self):
        for job in ('../outside', self.job + '/x', '', 'x' * 100):
            with self.assertRaises(ValueError):
                common.job_path(self.root, job)
        for size in (True, -1, 0, common.MAX_SOURCE + 1):
            with self.assertRaises(ValueError):
                common.metadata({'size': size, 'sha256': 'a' * 64}, common.MAX_SOURCE)

    def test_submit_is_idempotent_and_conflicts_preserved(self):
        self.submit()
        self.submit()
        with self.assertRaisesRegex(ValueError, 'different source'):
            self.submit(source_zip({'crates/new.rs': 'new'}))

    def test_claim_is_exclusive_and_same_worker_can_resume(self):
        self.submit()
        token = self.claim()
        self.assertIsNone(self.dispatch('claim', worker='b' * 32)['job'])
        self.assertEqual(self.claim(), token)

    def test_all_three_artifacts_and_windows_audit_required_for_success(self):
        self.submit()
        token = self.claim()
        files = self.windows_success()
        with self.assertRaisesRegex(ValueError, 'Both platforms'):
            self.dispatch('complete', token=token, verified_windows=files)
        self.upload_mac(token)
        with self.assertRaisesRegex(ValueError, 'both Windows'):
            self.dispatch('complete', token=token, verified_windows={})
        result = self.dispatch('complete', token=token, verified_windows=files)
        self.assertEqual(result['state'], 'succeeded')
        receipt = common.read_json(common.job_path(self.root, self.job) / 'receipt.json')
        self.assertEqual(len(receipt['files']), 3)
        self.assertFalse(receipt['published'])
        self.assertFalse(receipt['macos_notarized'])
        self.assertFalse(receipt['installation_tested'])

    def test_crash_after_mac_upload_can_resume_final_verification(self):
        self.submit()
        token = self.claim()
        self.upload_mac(token)
        work = self.root / 'work'
        common.write_json(work / 'active.json', {'job': self.job})
        calls = []
        def rpc(config, request):
            calls.append(request['action'])
            return gateway.dispatch(self.root, request, io.BytesIO())[0]
        with patch.object(client, 'check_mac'), patch.object(client, 'rpc', side_effect=rpc), \
             patch.object(client, 'process_job') as process:
            client.worker({'work_dir': work, 'worker': self.worker}, once=True)
        self.assertEqual(calls, ['status'])
        self.assertEqual(process.call_args[0][1]['macos']['token'], token)
        self.assertEqual(process.call_args[0][1]['macos']['state'], 'succeeded')
        self.assertFalse((work / 'active.json').exists())

    def test_failed_windows_build_is_not_complete(self):
        self.submit()
        token = self.claim()
        directory = common.job_path(self.root, self.job)
        common.write_json(directory / 'windows.json', {'state': 'failed', 'error': 'isolated failure'})
        self.upload_mac(token)
        self.assertEqual(common.status(self.root, self.job)['state'], 'failed')
        with self.assertRaisesRegex(ValueError, 'Both platforms'):
            self.dispatch('complete', token=token, verified_windows={})

    def test_wrong_worker_token_cannot_write_results(self):
        self.submit()
        self.claim()
        with self.assertRaisesRegex(ValueError, 'another worker'):
            self.upload_mac('wrong')

    def test_successful_mac_retry_clears_previous_error(self):
        self.submit()
        token = self.claim()
        self.dispatch('fail-macos', token=token, error='previous failure')
        result = self.upload_mac(token)
        self.assertEqual(result['macos']['state'], 'succeeded')
        self.assertNotIn('error', result['macos'])

    def test_mac_image_ignores_temporary_and_other_version_images(self):
        bundle = self.root / 'bundle'
        (bundle / 'dmg').mkdir(parents=True)
        for name in ('rw.123.CView_1.2.3_aarch64.dmg', 'CView_1.2.2_aarch64.dmg', 'CView_1.2.3_aarch64.dmg'):
            (bundle / 'dmg' / name).write_bytes(b'isolated test fixture')
        self.assertEqual(client.mac_image(bundle, '1.2.3').name, 'CView_1.2.3_aarch64.dmg')
        with self.assertRaisesRegex(ValueError, 'missing'):
            client.mac_image(bundle, '1.2.4')

    def test_download_requires_exact_artifact_name_and_completion(self):
        self.submit()
        for name in ('../../setup.log', 'CView_1.2.3_x86-setup.exe'):
            with self.assertRaisesRegex(ValueError, 'Invalid artifact'):
                self.dispatch('artifact', name=name)
        with self.assertRaisesRegex(ValueError, 'not complete'):
            self.dispatch('artifact', name='CView_1.2.3_x64-setup.exe')

    def test_server_rechecks_stored_artifact_hash(self):
        self.submit()
        self.windows_success()
        path = common.job_path(self.root, self.job) / 'artifacts/CView_1.2.3_x64-setup.exe'
        path.write_bytes(b'changed-on-disk')
        with self.assertRaisesRegex(ValueError, 'integrity'):
            self.dispatch('artifact', name=path.name)

    def test_client_rejects_corrupted_binary_download(self):
        def respond(command, **kwargs):
            kwargs['stdout'].write((json.dumps({'ok': True, 'file': info(b'expected')}) + '\n').encode() + b'corrupt!')
            return subprocess.CompletedProcess(command, 0, stderr=b'')
        config = {'ssh_key': '/unused', 'known_hosts': '/unused', 'ssh_target': 'unused'}
        with patch.object(client.subprocess, 'run', side_effect=respond), self.assertRaisesRegex(ValueError, 'SHA-256'):
            client.rpc(config, {'action': 'source'}, download=self.root / 'download')
        self.assertFalse((self.root / 'download').exists())

    def test_tailnet_proxy_keeps_key_and_host_pinning(self):
        config = {'ssh_key': '/private/key', 'known_hosts': '/private/known_hosts',
                  'ssh_target': 'builder@host', 'tailscale_nc': '/path with spaces/tailscale'}
        args = client.ssh_command(config)
        self.assertIn("ProxyCommand='/path with spaces/tailscale' nc %h %p", args)
        self.assertIn('StrictHostKeyChecking=yes', args)
        self.assertIn('UserKnownHostsFile=/private/known_hosts', args)
        self.assertIn('BatchMode=yes', args)
        self.assertEqual(args[-3:], ['--', 'builder@host', common.PROTOCOL])

    def test_client_reports_mac_failure_instead_of_completion(self):
        self.submit()
        self.claim()
        job = common.status(self.root, self.job)
        calls = []
        with patch.object(client, 'check_mac', side_effect=ValueError('missing Mac tool')), \
             patch.object(client, 'rpc', side_effect=lambda config, request: calls.append(request)):
            with self.assertRaisesRegex(ValueError, 'missing Mac tool'):
                client.process_job({'work_dir': self.root / 'work'}, job)
        self.assertEqual([r['action'] for r in calls], ['fail-macos'])

    def test_native_log_keeps_stderr_and_real_failure_exit_code(self):
        log = self.root / 'windows.log'
        code = 'import sys; print("stdout marker", flush=True); print("stderr marker", file=sys.stderr); sys.exit(7)'
        self.assertEqual(runner.native_logged(log, [sys.executable, '-c', code]), 7)
        data = log.read_text()
        self.assertIn('stdout marker', data)
        self.assertIn('stderr marker', data)

    def _lane_starter(self, scripts):
        def start(name, lane_log):
            code = f'import sys, time; open({str(lane_log)!r}, "a").write("lane {name} marker\\n"); ' + scripts[name]
            return subprocess.Popen([sys.executable, '-c', code])
        return start

    def test_build_lanes_run_concurrently_and_merge_logs_in_order(self):
        log = self.root / 'windows.log'
        scripts = {name: 'time.sleep(0.8)' for name, _ in common.LANES}
        started = time.monotonic()
        failed = runner.run_lanes([n for n, _ in common.LANES], log, self._lane_starter(scripts), poll=0.05)
        self.assertEqual(failed, [])
        self.assertLess(time.monotonic() - started, 2.0, 'lanes must overlap, not run one after another')
        data = log.read_text()
        positions = [data.index(f'===== lane {n}: ok') for n, _ in common.LANES]
        self.assertEqual(positions, sorted(positions))
        for name, _ in common.LANES:
            self.assertIn(f'lane {name} marker', data)
        self.assertIn('Lanes: check', data)

    def test_first_failing_lane_stops_the_others(self):
        log = self.root / 'windows.log'
        scripts = {'check': 'sys.exit(3)', 'x64': 'time.sleep(30)', 'arm64': 'time.sleep(30)'}
        started = time.monotonic()
        failed = runner.run_lanes(['check', 'x64', 'arm64'], log, self._lane_starter(scripts), poll=0.05)
        self.assertLess(time.monotonic() - started, 10, 'a failed check must not wait for both release builds')
        self.assertEqual(failed, ['check', 'x64', 'arm64'])
        data = log.read_text()
        self.assertIn('lane check: failed (exit 3)', data)
        self.assertIn('lane x64: stopped', data)

    def test_lanes_use_separate_cargo_target_dirs(self):
        dirs = [d for _, d in common.LANES]
        self.assertEqual(len(set(d.casefold() for d in dirs)), len(dirs))
        self.assertEqual(common.LANE_TARGET_DIRS['x64'], 'target', 'x64 keeps the existing warm cache')
        self.assertEqual(set(common.TARGETS), {'x64', 'arm64'} & set(common.LANE_TARGET_DIRS))
        script = (Path(runner.__file__).with_name('Build-Windows.ps1')).read_text()
        self.assertIn("[ValidateSet('all', 'check', 'x64', 'arm64')]", script)
        self.assertIn('$TargetDir', script)

    def test_native_log_reports_failure_to_start(self):
        log = self.root / 'windows.log'
        self.assertEqual(runner.native_logged(log, [str(self.root / 'not-installed')]), 1)
        self.assertIn('Cannot start native command', log.read_text())

    def test_windows_release_skips_mac_worker_and_requires_both_audits(self):
        self.submit(mode=common.WINDOWS_RELEASE_MODE)
        self.assertIsNone(self.dispatch('claim', worker=self.worker)['job'])
        files = self.windows_success()
        self.assertEqual(common.status(self.root, self.job)['macos']['state'], 'external')
        self.assertEqual(common.status(self.root, self.job)['state'], 'pending')
        with self.assertRaisesRegex(ValueError, 'both Windows'):
            self.dispatch('complete-windows', verified_windows={})
        with self.assertRaisesRegex(ValueError, 'formal release'):
            self.upload_mac('unused')
        self.assertEqual(self.dispatch('complete-windows', verified_windows=files)['state'], 'succeeded')
        receipt = common.job_path(self.root, self.job) / 'receipt.json'
        before = receipt.read_bytes()
        self.dispatch('complete-windows', verified_windows=files)
        self.assertEqual(receipt.read_bytes(), before)
        self.assertEqual(len(json.loads(before)['files']), 2)
        self.assertFalse(json.loads(before)['published'])

    def test_windows_completion_rejects_build_only_and_corrupt_server_artifacts(self):
        self.submit()
        with self.assertRaisesRegex(ValueError, 'not a Windows release'):
            self.dispatch('complete-windows', verified_windows={})
        self.job = common.job_id()
        self.submit(mode=common.WINDOWS_RELEASE_MODE)
        files = self.windows_success()
        directory = common.job_path(self.root, self.job)
        (directory / 'artifacts/CView_1.2.3_arm64-setup.exe').write_bytes(b'corrupted')
        with self.assertRaisesRegex(ValueError, 'Stored artifact'):
            self.dispatch('complete-windows', verified_windows=files)
        self.assertFalse((directory / 'receipt.json').exists())

    def test_job_mode_and_compile_environment_cannot_change_on_retry(self):
        self.submit(mode=common.WINDOWS_RELEASE_MODE, build_env={'CVIEW_UPDATE_URL': 'https://example.test/feed.json'})
        for options in ({}, {'mode': common.WINDOWS_RELEASE_MODE},
                        {'mode': common.WINDOWS_RELEASE_MODE, 'build_env': {'CVIEW_UPDATE_URL': 'https://other.test/'}}):
            with self.subTest(options=options), self.assertRaisesRegex(ValueError, 'mode or environment'):
                self.submit(**options)

    def test_only_public_update_url_can_be_forwarded(self):
        for value in ({'APPLE_PASSWORD': 'must-never-forward'}, {'PATH': '/arbitrary'},
                      {'CVIEW_UPDATE_URL': 'http://example.test/'},
                      {'CVIEW_UPDATE_URL': 'https://user:pass@example.test/feed'},
                      {'CVIEW_UPDATE_URL': 'https://example.test/\n'}):
            with self.subTest(value=value), self.assertRaises(ValueError):
                common.build_environment(value)
        self.assertEqual(common.build_environment({'CVIEW_UPDATE_URL': ''}), {'CVIEW_UPDATE_URL': ''})

    def test_release_preflight_rejects_old_gateway_and_wrong_server(self):
        response = {'protocol': common.PROTOCOL, 'server_name': 'EXPECTED'}
        config = {'readobj': self.root / 'readobj', 'server_name': 'expected'}
        with patch.object(client.shutil, 'which', return_value='/installed/7zz'), \
             patch.object(client.os, 'access', return_value=True), patch.object(client, 'rpc', return_value=response):
            with self.assertRaisesRegex(ValueError, 'redeploy'):
                client.preflight(config, windows_release=True)
            response['features'] = [common.WINDOWS_RELEASE_FEATURE]
            response['server_name'] = 'OTHER'
            with self.assertRaisesRegex(ValueError, 'Wrong build server'):
                client.preflight(config, windows_release=True)

    def release_fixture(self):
        archive = self.root / 'fixture.zip'
        archive.write_bytes(source_zip())
        source = self.root / 'checkout'
        common.validate_archive(archive, source)
        return source, {'work_dir': self.root / 'work', 'readobj': self.root / 'readobj'}

    def release_rpc(self, calls, outcome='succeeded', corrupt=False):
        def rpc(config, request, upload=None, download=None):
            calls.append(request['action'])
            if request['action'] == 'submit':
                with upload.open('rb') as stream:
                    result = common.submit(self.root, request['job'], stream, request['source'], start=False,
                                           mode=request['mode'], build_env=request['build_env'])
                self.job = request['job']
                if outcome == 'succeeded':
                    self.windows_success()
                elif outcome == 'failed':
                    common.write_json(common.job_path(self.root, self.job) / 'windows.json',
                                      {'state': 'failed', 'error': 'compiler failed'})
                (common.job_path(self.root, self.job) / 'windows.log').write_text('isolated build log')
                return common.status(self.root, self.job)
            result, path = gateway.dispatch(self.root, request, io.BytesIO())
            if download:
                data = path.read_bytes()
                if corrupt and request['action'] == 'artifact':
                    data = b'x' * len(data)
                common.receive(io.BytesIO(data), download, result['file'])
            return result
        return rpc

    def test_release_client_exports_only_after_both_installer_audits(self):
        source, config = self.release_fixture()
        output, calls, audits = self.root / 'dist', [], []
        def audit(command, cwd, env, log):
            self.assertNotIn('complete-windows', calls)
            self.assertFalse(output.exists())
            audits.append(command[command.index('--machine') + 1])
        with patch.object(client, 'preflight'), patch.object(client, 'rpc', side_effect=self.release_rpc(calls)), \
             patch.object(client, 'run', side_effect=audit), patch.dict(os.environ,
                 {'APPLE_PASSWORD': 'do-not-forward', 'CVIEW_UPDATE_URL': 'https://example.test/feed.json'}, clear=True):
            receipt = client.release_windows(config, source, '1.2.3', output)
        self.assertEqual(audits, ['0x8664', '0xaa64'])
        self.assertNotIn('claim', calls)
        self.assertNotIn('upload-macos', calls)
        self.assertEqual(receipt['build_env'], {'CVIEW_UPDATE_URL': 'https://example.test/feed.json'})
        for name, expected in receipt['files'].items():
            self.assertEqual(common.describe(output / name), expected)
        self.assertEqual(common.read_json(output / 'CView_1.2.3_windows-build.json'), receipt)

    def test_windows_failure_or_corrupt_download_keeps_existing_outputs(self):
        source, config = self.release_fixture()
        output = self.root / 'dist'
        output.mkdir()
        original = output / 'CView_1.2.3_x64-setup.exe'
        original.write_bytes(b'previous successful output')
        for outcome, corrupt, message in [('failed', False, 'compiler failed'), ('succeeded', True, 'SHA-256')]:
            calls = []
            with self.subTest(outcome=outcome), patch.object(client, 'preflight'), \
                 patch.object(client, 'rpc', side_effect=self.release_rpc(calls, outcome, corrupt)), \
                 patch.object(client, 'run'), self.assertRaisesRegex((ValueError, RuntimeError), message):
                client.release_windows(config, source, '1.2.3', output)
            self.assertNotIn('complete-windows', calls)
            self.assertEqual(original.read_bytes(), b'previous successful output')
            self.assertFalse((output / 'CView_1.2.3_windows-build.json').exists())

    def test_arm64_audit_failure_prevents_export_and_receipt(self):
        source, config = self.release_fixture()
        calls, output = [], self.root / 'dist'
        def audit(command, *args):
            if '0xaa64' in command:
                raise subprocess.CalledProcessError(1, command)
        with patch.object(client, 'preflight'), patch.object(client, 'rpc', side_effect=self.release_rpc(calls)), \
             patch.object(client, 'run', side_effect=audit), self.assertRaises(subprocess.CalledProcessError):
            client.release_windows(config, source, '1.2.3', output)
        self.assertNotIn('complete-windows', calls)
        self.assertFalse(output.exists())
        self.assertFalse((common.job_path(self.root, self.job) / 'receipt.json').exists())

    def test_release_source_version_mismatch_stops_before_upload(self):
        source, config = self.release_fixture()
        with patch.object(client, 'preflight'), patch.object(client, 'rpc') as rpc, \
             self.assertRaisesRegex(ValueError, 'version differs'):
            client.release_windows(config, source, '1.2.4', self.root / 'dist')
        rpc.assert_not_called()

    def test_release_resume_downloads_existing_job_without_rebuilding(self):
        source, config = self.release_fixture()
        self.submit(mode=common.WINDOWS_RELEASE_MODE)
        self.windows_success()
        (common.job_path(self.root, self.job) / 'windows.log').write_text('existing build log')
        calls = []
        with patch.object(client, 'preflight'), patch.object(client, 'rpc', side_effect=self.release_rpc(calls)), \
             patch.object(client, 'run'), patch.dict(os.environ, {}, clear=True):
            receipt = client.release_windows(config, source, '1.2.3', self.root / 'dist', job=self.job)
        self.assertNotIn('submit', calls)
        self.assertNotIn('claim', calls)
        self.assertEqual(len(receipt['files']), 2)
        self.assertEqual(receipt['job'], self.job)

    def test_release_timeout_preserves_job_without_export(self):
        source, config = self.release_fixture()
        calls = []
        with patch.object(client, 'preflight'), patch.object(client, 'rpc', side_effect=self.release_rpc(calls, 'queued')), \
             patch.object(client.time, 'monotonic', side_effect=[0, 1]), self.assertRaisesRegex(TimeoutError, 'preserved'):
            client.release_windows(config, source, '1.2.3', self.root / 'dist', timeout=1)
        self.assertNotIn('complete-windows', calls)
        self.assertTrue((common.job_path(self.root, self.job) / 'source.zip').exists())
        self.assertFalse((self.root / 'dist').exists())


if __name__ == '__main__':
    unittest.main()

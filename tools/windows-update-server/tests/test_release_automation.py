import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
import release_gateway as gateway
import publish

spec = importlib.util.spec_from_file_location('publish_update', TOOLS.parents[1] / 'scripts/publish-update.py')
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)


class GatewayTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'incoming').mkdir()
        (self.root / 'tools').mkdir()
        for name in ['publish.py', 'check_server.py']:
            shutil.copyfile(TOOLS / name, self.root / 'tools' / name)
        self.version = '0.1.1'

    def call(self, request, data=b''):
        return gateway.dispatch(request, io.BytesIO(data), self.root)

    def upload(self, name, data=b'isolation-test-only'):
        info = {'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
        self.call({'action': 'upload', 'version': self.version, 'name': name, **info}, data)
        return info

    def stage(self):
        return {name: self.upload(name, ('isolated ' + name + '\n').encode()) for name in gateway.filenames(self.version)}

    def test_path_traversal_and_arbitrary_names_rejected(self):
        for name in ['../escape.exe', 'notes.txt:evil', 'publish.py', 'CView_0.1.1_local.dmg']:
            with self.assertRaises(ValueError):
                self.upload(name)
        self.assertFalse((self.root / 'incoming' / self.version).exists())
        for version in ['../1.1', '01.1.1', '1.0.0;hostname', '1.0.0\n', '1.0.0-beta']:
            with self.assertRaises(ValueError):
                gateway.filenames(version)

    def test_hash_mismatch_never_promotes_file(self):
        name = gateway.filenames(self.version)[0]
        with self.assertRaisesRegex(ValueError, 'SHA-256'):
            self.call({'action': 'upload', 'version': self.version, 'name': name,
                       'size': 3, 'sha256': '0' * 64}, b'abc')
        self.assertEqual(list((self.root / 'incoming' / self.version).iterdir()), [])

    def test_truncated_upload_cleans_temporary_files(self):
        name = gateway.filenames(self.version)[0]
        with self.assertRaisesRegex(ValueError, 'Truncated'):
            self.call({'action': 'upload', 'version': self.version, 'name': name,
                       'size': 100, 'sha256': '0' * 64}, b'a')
        self.assertEqual(list((self.root / 'incoming' / self.version).iterdir()), [])

    def test_upload_retry_is_idempotent_and_conflicts_preserved(self):
        name = gateway.filenames(self.version)[0]
        original = self.upload(name)
        self.assertEqual(self.upload(name), original)
        with self.assertRaisesRegex(ValueError, 'Staged file differs'):
            self.upload(name, b'different')
        self.assertEqual(gateway.describe(self.root / 'incoming' / self.version / name), original)

    def test_symlink_escape_blocked(self):
        (self.root / 'incoming' / self.version).symlink_to(self.root / 'tools', target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'Links'):
            self.upload(gateway.filenames(self.version)[0])

    def test_validation_requires_both_platforms_and_matching_files(self):
        files = self.stage()
        request = {'action': 'validate', 'version': self.version, 'files': files}
        self.assertFalse(self.call(request)['published'])
        self.assertFalse((self.root / 'site').exists())
        files.pop(gateway.filenames(self.version)[1])
        with self.assertRaisesRegex(ValueError, 'Both installers'):
            self.call(request)

    def test_tampering_after_upload_blocks_publication(self):
        files = self.stage()
        (self.root / 'incoming' / self.version / 'notes.txt').write_bytes(b'tampered')
        with self.assertRaisesRegex(ValueError, 'Staged file mismatch'):
            self.call({'action': 'publish', 'version': self.version, 'files': files})
        self.assertFalse((self.root / 'site').exists())

    def test_publish_retry_verifies_without_overwriting(self):
        files = self.stage()
        request = {'action': 'publish', 'version': self.version, 'files': files}
        # Only server HTTPS checking is mocked; the real publisher writes to a disposable local root.
        with patch.object(gateway.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'', b'')):
            first = self.call(request)
            before = (self.root / 'site/releases/latest.json').read_bytes()
            second = self.call(request)
        self.assertFalse(first['reused'])
        self.assertTrue(second['reused'])
        self.assertEqual(before, (self.root / 'site/releases/latest.json').read_bytes())
        with self.assertRaisesRegex(ValueError, 'newer'):
            self.upload(gateway.filenames(self.version)[0])
        files['notes.txt']['sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'differs'):
            self.call(request)

    def test_published_but_verification_failed_can_be_retried(self):
        files = self.stage()
        request = {'action': 'publish', 'version': self.version, 'files': files}
        with patch.object(gateway.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, b'', b'network failure')):
            with self.assertRaisesRegex(RuntimeError, 'Published, but'):
                self.call(request)
        with patch.object(gateway.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'', b'')):
            self.assertTrue(self.call(request)['reused'])

    def test_probe_readback_and_cleanup(self):
        data = b'P' * (1024 * 1024)
        result = self.call({'action': 'probe', 'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()}, data)
        self.assertTrue(result['cleaned'])
        self.assertEqual(list((self.root / 'incoming').iterdir()), [])
        self.assertFalse((self.root / 'site').exists())

    def test_unsupported_action_does_not_execute(self):
        with self.assertRaisesRegex(ValueError, 'Unsupported'):
            self.call({'action': 'powershell', 'command': 'hostname'})

    def test_concurrent_operations_rejected(self):
        with gateway.locked(self.root):
            with self.assertRaises(OSError):
                self.call({'action': 'preflight'})


class ClientTests(unittest.TestCase):
    def test_ssh_does_not_reuse_unrestricted_master_or_password(self):
        with tempfile.NamedTemporaryFile() as key, tempfile.NamedTemporaryFile() as hosts, patch.dict('os.environ', {
                'CVIEW_RELEASE_SSH_KEY': key.name, 'CVIEW_RELEASE_KNOWN_HOSTS': hosts.name,
                'CVIEW_RELEASE_SSH_TARGET': 'user@release.example', 'CVIEW_RELEASE_SERVER_NAME': 'SERVER'}):
            os.chmod(key.name, 0o600)
            command = client.ssh_command()
        for option in ['BatchMode=yes', 'ControlPath=none', 'StrictHostKeyChecking=yes', 'PreferredAuthentications=publickey',
                       'UserKnownHostsFile=' + hosts.name]:
            self.assertIn(option, command)
        self.assertEqual(command[-3:], ['--', 'user@release.example', gateway.PROTOCOL])

    def test_server_settings_come_from_local_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp)
            (config / 'server.json').write_text(json.dumps({'ssh_target': 'user@release.example', 'server_name': 'SERVER'}), encoding='utf-8')
            (config / 'known_hosts').write_text('release.example ssh-ed25519 AAAA\n', encoding='utf-8')
            clean = {k: v for k, v in os.environ.items() if not k.startswith('CVIEW_RELEASE_')}
            with patch.object(client, 'CONFIG_DIR', config), patch.dict('os.environ', clean, clear=True):
                settings = client.server_settings()
            self.assertEqual(settings['ssh_target'], 'user@release.example')
            self.assertEqual(settings['known_hosts'], config / 'known_hosts')

    def test_missing_server_settings_fail_clearly(self):
        with tempfile.TemporaryDirectory() as tmp:
            clean = {k: v for k, v in os.environ.items() if not k.startswith('CVIEW_RELEASE_')}
            with patch.object(client, 'CONFIG_DIR', Path(tmp)), patch.dict('os.environ', clean, clear=True):
                with self.assertRaisesRegex(ValueError, 'not configured'):
                    client.server_settings()

    def test_ssh_failure_exits_nonzero(self):
        with patch.object(sys, 'argv', ['publish-update.py', '--preflight']), patch.object(client, 'rpc', side_effect=RuntimeError('offline')):
            self.assertEqual(client.main(), 1)

    def test_cross_origin_redirect_rejected(self):
        with self.assertRaisesRegex(ValueError, 'redirect'):
            client.SameOriginRedirect().redirect_request(None, None, 302, '', {}, 'http://example.org/file')


if __name__ == '__main__':
    unittest.main()

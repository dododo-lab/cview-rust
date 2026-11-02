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

VECTOR = json.loads((Path(__file__).with_name('signed_feed_vector.json')).read_text(encoding='utf-8'))
# 서버는 형식만 보므로 게이트웨이 테스트에는 테스트 벡터 서명을 그대로 쓴다.
SIGNATURE = VECTOR['manifest']['signature']


def has_openssl():
    try:
        client.openssl()
        return True
    except ValueError:
        return False


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
        request = {'action': 'validate', 'version': self.version, 'files': files, 'signature': SIGNATURE}
        self.assertFalse(self.call(request)['published'])
        self.assertFalse((self.root / 'site').exists())
        files.pop(gateway.filenames(self.version)[1])
        with self.assertRaisesRegex(ValueError, 'Both installers'):
            self.call(request)

    def test_tampering_after_upload_blocks_publication(self):
        files = self.stage()
        (self.root / 'incoming' / self.version / 'notes.txt').write_bytes(b'tampered')
        with self.assertRaisesRegex(ValueError, 'Staged file mismatch'):
            self.call({'action': 'publish', 'version': self.version, 'files': files, 'signature': SIGNATURE})
        self.assertFalse((self.root / 'site').exists())

    def test_publish_retry_verifies_without_overwriting(self):
        files = self.stage()
        request = {'action': 'publish', 'version': self.version, 'files': files, 'signature': SIGNATURE}
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
        request = {'action': 'publish', 'version': self.version, 'files': files, 'signature': SIGNATURE}
        with patch.object(gateway.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, b'', b'network failure')):
            with self.assertRaisesRegex(RuntimeError, 'Published, but'):
                self.call(request)
        with patch.object(gateway.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'', b'')):
            self.assertTrue(self.call(request)['reused'])

    def test_publish_requires_signature_and_writes_it_to_feed(self):
        files = self.stage()
        request = {'action': 'publish', 'version': self.version, 'files': files}
        bad = [None, {}, {**SIGNATURE, 'alg': 'rsa'}, {**SIGNATURE, 'key_id': 'XYZ'},
               {**SIGNATURE, 'value': 'short=='}, {**SIGNATURE, 'extra': 1}]
        for signature in bad:
            with self.assertRaisesRegex(ValueError, 'signature'):
                self.call({**request, 'signature': signature})
        self.assertFalse((self.root / 'site').exists())
        with patch.object(gateway.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'', b'')):
            self.call({**request, 'signature': SIGNATURE})
            feed = json.loads((self.root / 'site/releases/latest.json').read_text(encoding='utf-8'))
            self.assertEqual(feed['signature'], SIGNATURE)
            other = {**SIGNATURE, 'key_id': '0' * 16}
            with self.assertRaisesRegex(ValueError, 'signature differs'):
                self.call({**request, 'signature': other})

    def test_preflight_announces_signed_feed(self):
        self.assertIn('signed-feed-v1', self.call({'action': 'preflight'})['features'])

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

    def test_notes_must_not_require_tailscale_for_users(self):
        base = 'CView 0.1.1\nWindows: 미서명, SmartScreen 경고\n'
        client.check_notes(base)
        with self.assertRaisesRegex(ValueError, 'Tailscale'):
            client.check_notes(base + '업데이트 확인과 다운로드에는 Tailscale 연결이 필요합니다.\n')
        with self.assertRaisesRegex(ValueError, 'SmartScreen'):
            client.check_notes('CView 0.1.1\n')
        with self.assertRaisesRegex(ValueError, '공증 안 함'):
            client.check_notes(base, allow_unnotarized=True)

    def test_generated_notes_say_updates_are_public(self):
        script = (TOOLS.parents[1] / 'scripts/release-macos.sh').read_text(encoding='utf-8')
        self.assertNotIn('Tailscale 연결이 필요', script)
        self.assertIn('Tailscale 불필요', script)

    def test_outdated_gateway_blocks_publishing(self):
        settings = {'ssh_target': 'user@release.example', 'server_name': 'SERVER', 'known_hosts': Path('/dev/null')}
        state = {'protocol': gateway.PROTOCOL, 'hostname': 'server', 'latest': None, 'free_bytes': 1}
        with patch.object(sys, 'argv', ['publish-update.py', '--preflight']), \
                patch.object(client, 'server_settings', return_value=settings), \
                patch.object(client, 'rpc', return_value=state), \
                patch.object(client, 'verify_https', side_effect=AssertionError('must stop before HTTPS')), \
                patch('sys.stderr', new_callable=io.StringIO) as stderr:
            self.assertEqual(client.main(), 1)
        self.assertIn('redeploy', stderr.getvalue())

    def test_cross_origin_redirect_rejected(self):
        with self.assertRaisesRegex(ValueError, 'redirect'):
            client.SameOriginRedirect().redirect_request(None, None, 302, '', {}, 'http://example.org/file')


class SignatureTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.keys = self.dir / 'keys.txt'
        self.keys.write_text('# test\n' + VECTOR['public_key'] + '\n', encoding='utf-8')
        for name, value in [('TRUSTED_KEYS', self.keys), ('SIGNING_KEY', self.dir / 'config' / 'signing.pem')]:
            patcher = patch.object(client, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_payload_matches_app_vector(self):
        # Rust(updater.rs signed_payload_matches_python)도 같은 벡터를 확인한다.
        self.assertEqual(gateway.signed_payload(VECTOR['manifest']).decode('utf-8'), VECTOR['payload'])
        for field in ['name', 'browser_download_url']:
            manifest = json.loads(json.dumps(VECTOR['manifest']))
            manifest['assets'][0][field] += ' x'
            with self.assertRaisesRegex(ValueError, 'whitespace'):
                gateway.signed_payload(manifest)

    def test_planned_manifest_matches_published_feed(self):
        # 클라이언트가 서명한 내용과 서버 publish.py 가 실제로 쓰는 피드가 같은 서명 대상을 만들어야 한다.
        version = '0.1.69'
        sources = []
        for name in gateway.filenames(version)[:2]:
            path = self.dir / name
            path.write_bytes(b'artifact ' + name.encode())
            sources.append(path)
        expected = {path.name: gateway.describe(path) for path in sources}
        notes = 'CView 0.1.69\n\n변경 사항\n'
        published = publish.publish(self.dir / 'site', gateway.BASE, version, sources, notes, SIGNATURE)
        self.assertEqual(gateway.signed_payload(client.planned_manifest(version, notes, expected)),
                         gateway.signed_payload(published))

    @unittest.skipUnless(has_openssl(), 'OpenSSL 3+ not installed')
    def test_vector_signature_verifies_and_tampering_fails(self):
        client.verify_signature(VECTOR['manifest'])
        tampered = json.loads(json.dumps(VECTOR['manifest']))
        tampered['assets'][1]['sha256'] = '2' * 64
        with self.assertRaisesRegex(ValueError, 'verification failed'):
            client.verify_signature(tampered)
        with self.assertRaisesRegex(ValueError, 'signature'):
            client.verify_signature({k: v for k, v in VECTOR['manifest'].items() if k != 'signature'})
        self.keys.write_text('# empty\n', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'untrusted'):
            client.verify_signature(VECTOR['manifest'])

    @unittest.skipUnless(has_openssl(), 'OpenSSL 3+ not installed')
    def test_signing_key_lifecycle(self):
        with self.assertRaisesRegex(ValueError, 'missing'):
            client.signing_public_key()
        with patch('sys.stdout', new_callable=io.StringIO) as out:
            client.init_signing_key()
        self.assertEqual(client.SIGNING_KEY.stat().st_mode & 0o777, 0o600)
        with self.assertRaisesRegex(ValueError, 'never overwrite'):
            client.init_signing_key()
        manifest = {k: v for k, v in VECTOR['manifest'].items() if k != 'signature'}
        # 저장소 목록에 없는 키로는 서명하지 않는다(앱이 거부할 피드를 게시하지 않는다).
        with self.assertRaisesRegex(ValueError, 'not in'):
            client.sign_manifest(manifest)
        public = out.getvalue().strip().splitlines()[-1]
        self.keys.write_text(self.keys.read_text(encoding='utf-8') + public + '\n', encoding='utf-8')
        signature = client.sign_manifest(manifest)
        self.assertEqual(gateway.checked_signature(signature), signature)
        client.verify_signature({**manifest, 'signature': signature})
        os.chmod(client.SIGNING_KEY, 0o644)
        with self.assertRaisesRegex(ValueError, 'chmod 600'):
            client.sign_manifest(manifest)

    def test_repository_key_list_is_well_formed(self):
        with patch.object(client, 'TRUSTED_KEYS', client.REPO / 'crates/cview-app/update-signing-keys.txt'):
            client.trusted_keys()


if __name__ == '__main__':
    unittest.main()

"""Test release orchestration with mocked macOS tools; no real DMG is built."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
MOCK = r'''
import json, os, sys
from pathlib import Path
name = Path(sys.argv[0]).name
args = sys.argv[1:]
if name == 'uname':
    print('Darwin' if args == ['-s'] else os.environ.get('TEST_ARCH', 'arm64'))
elif name == 'security':
    print('Developer ID Application: Test (TESTTEAM)')
elif name == 'cargo' and 'build' in args:
    assert args[-2:] == ['--', '--locked'], args
    target = args[args.index('--target') + 1]
    bundle = Path(os.environ['CARGO_TARGET_DIR']) / target / 'release/bundle'
    executable = bundle / 'macos/CView.app/Contents/MacOS/cview-app'
    executable.parent.mkdir(parents=True, exist_ok=True)
    executable.write_text('mock executable')
    if not os.environ.get('TEST_STALE'):
        dmg = bundle / 'dmg/CView.dmg'
        dmg.parent.mkdir(parents=True, exist_ok=True)
        dmg.write_text('mock disk image for orchestration tests only')
elif name == 'codesign' and os.environ.get('TEST_BAD_SIGNATURE'):
    sys.exit(1)
elif name == 'xcrun' and args[0] == 'notarytool':
    print(json.dumps({'status': os.environ.get('TEST_NOTARY', 'Accepted')}))
'''


@unittest.skipUnless(shutil.which('zsh'), 'zsh is required')
class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'scripts').mkdir()
        shutil.copy2(ROOT / 'scripts/release-macos.sh', self.root / 'scripts')
        config = self.root / 'crates/cview-app'
        config.mkdir(parents=True)
        (config / 'tauri.conf.json').write_text(json.dumps({
            'version': '0.1.0', 'bundle': {'macOS': {'signingIdentity':
                'Developer ID Application: Test (TESTTEAM)'}}}))
        bins = self.root / 'bin'
        bins.mkdir()
        mock = bins / 'mock'
        mock.write_text(f'#!{sys.executable}\n' + MOCK)
        mock.chmod(0o755)
        for name in ['uname', 'security', 'cargo', 'xcode-select', 'codesign',
                     'lipo', 'spctl', 'xcrun', 'hdiutil']:
            (bins / name).symlink_to(mock)
        self.env = {**os.environ, 'PATH': f'{bins}:{os.environ["PATH"]}',
                    'CARGO_TARGET_DIR': 'target', 'APPLE_SIGNING_IDENTITY':
                    'Developer ID Application: Test (TESTTEAM)',
                    'CVIEW_NOTARIZE_PROFILE': 'test-profile'}

    def run_release(self, *args):
        return subprocess.run(['zsh', 'scripts/release-macos.sh', '--dmg', *args],
                              cwd=self.root, env=self.env, capture_output=True,
                              text=True, timeout=20)

    def test_architecture_names_and_checksums(self):
        for target, arch in [('aarch64-apple-darwin', 'arm64'),
                             ('x86_64-apple-darwin', 'x64'),
                             ('universal-apple-darwin', 'universal')]:
            with self.subTest(target=target):
                result = self.run_release('--target', target)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertTrue((self.root / f'dist/CView_0.1.0_{arch}.dmg.sha256').is_file())

    def test_stale_dmg_cannot_be_released(self):
        dmg = self.root / 'target/aarch64-apple-darwin/release/bundle/dmg/old.dmg'
        dmg.parent.mkdir(parents=True)
        dmg.write_text('stale artifact')
        os.utime(dmg, (1, 1))
        self.env['TEST_STALE'] = '1'
        self.assertNotEqual(self.run_release().returncode, 0)
        self.assertFalse((self.root / 'dist').exists())

    def test_rejected_notarization_fails(self):
        self.env['TEST_NOTARY'] = 'Invalid'
        self.assertNotEqual(self.run_release().returncode, 0)
        self.assertFalse(list(self.root.glob('dist/*.sha256')))

    def test_invalid_signature_fails(self):
        self.env['TEST_BAD_SIGNATURE'] = '1'
        self.assertNotEqual(self.run_release().returncode, 0)
        self.assertFalse((self.root / 'dist').exists())


if __name__ == '__main__':
    unittest.main()

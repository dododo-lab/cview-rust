"""Feed publication and real HTTP retrieval tests; Windows IIS is tested separately."""
import functools
import hashlib
import http.server
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from unittest.mock import patch
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('publish', HERE / 'publish.py')
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'site'
        self.asset = Path(self.temp.name) / 'CView_0.1.1_arm64.dmg'
        self.asset.write_bytes(b'test installation artifact\x00' * 100)

    def publish(self, version='0.1.1', notes='', artifacts=None):
        return publisher.publish(self.root, 'https://updates.example.com', version,
                                 artifacts or [self.asset], notes)

    def test_live_http_manifest_page_and_download(self):
        result = self.publish(notes='<script>alert("x")</script>')
        handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(self.root))
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f'http://127.0.0.1:{server.server_port}'
            def get(path):
                with urllib.request.urlopen(base + path) as response:
                    return response.read()
            manifest = json.loads(get('/releases/latest.json?_ts=123'))
            self.assertEqual(manifest['tag_name'], 'v0.1.1')
            data = get('/releases/0.1.1/' + self.asset.name)
            asset = manifest['assets'][0]
            self.assertEqual(hashlib.sha256(data).hexdigest(), asset['sha256'])
            self.assertEqual(len(data), asset['size'])
            page = get('/releases/0.1.1/').decode()
            self.assertNotIn('<script>', page)
            self.assertIn('&lt;script&gt;', page)
            self.assertEqual(result['html_url'], 'https://updates.example.com/releases/0.1.1/')
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_versions_immutable_and_downgrades_rejected(self):
        self.publish()
        before = (self.root / 'releases/latest.json').read_bytes()
        for version in ['0.1.1', '0.1.0']:
            with self.assertRaises((FileExistsError, ValueError)):
                self.publish(version)
        self.assertEqual((self.root / 'releases/latest.json').read_bytes(), before)

    def test_validation_does_not_change_feed(self):
        self.publish()
        before = (self.root / 'releases/latest.json').read_bytes()
        for version in ['../outside', '0.1.2-beta', '01.2.3']:
            with self.assertRaises(ValueError):
                self.publish(version)
        for name in ['CView_local.dmg', 'CView_local-build-source.zip', 'shell.ps1', 'empty.exe']:
            path = self.asset.parent / name
            path.write_bytes(b'' if name == 'empty.exe' else b'test')
            with self.assertRaises(ValueError):
                self.publish('0.1.2', artifacts=[path])
        self.assertEqual((self.root / 'releases/latest.json').read_bytes(), before)

    def test_failed_copy_keeps_latest_and_cleans_lock(self):
        self.publish()
        before = (self.root / 'releases/latest.json').read_bytes()
        with patch.object(publisher.shutil, 'copyfile', side_effect=OSError('disk full')):
            with self.assertRaises(OSError):
                self.publish('0.1.2')
        self.assertEqual((self.root / 'releases/latest.json').read_bytes(), before)
        self.assertFalse((self.root.parent / '.site-publish.lock').exists())
        self.assertFalse((self.root / 'releases/0.1.2').exists())

    def test_failed_feed_replacement_keeps_previous_release(self):
        self.publish()
        before = (self.root / 'releases/latest.json').read_bytes()
        with patch.object(publisher, 'atomic_write', side_effect=OSError('locked feed')):
            with self.assertRaises(OSError):
                self.publish('0.1.2')
        self.assertEqual((self.root / 'releases/latest.json').read_bytes(), before)
        self.assertFalse((self.root / 'releases/0.1.2').exists())
        self.publish('0.1.2')
        self.assertEqual(json.loads((self.root / 'releases/latest.json').read_text())['tag_name'], 'v0.1.2')

    def test_concurrent_publisher_lock_is_respected(self):
        lock = self.root.parent / '.site-publish.lock'
        lock.write_text('another publisher')
        with self.assertRaises(FileExistsError):
            self.publish()
        self.assertEqual(lock.read_text(), 'another publisher')

    def test_https_and_case_insensitive_names(self):
        for url in ['http://example.com', 'https://user:pass@example.com', 'https://example.com?x=1', 'file:///tmp']:
            with self.assertRaises(ValueError):
                publisher.public_base(url)
        second = self.asset.parent / self.asset.name.upper()
        second.write_bytes(b'test')
        with self.assertRaises(ValueError):
            self.publish(artifacts=[self.asset, second])

    def test_iis_feed_cache_and_mime_configuration(self):
        config = ET.parse(HERE / 'web.config').getroot()
        types = {e.attrib['fileExtension']: e.attrib['mimeType']
                 for e in config.findall('./system.webServer/staticContent/mimeMap')}
        self.assertEqual(types['.json'], 'application/json')
        self.assertIn('.dmg', types)
        feed = config.find('./location[@path="releases/latest.json"]')
        self.assertEqual(feed.find('./system.webServer/staticContent/clientCache').attrib['cacheControlMode'], 'DisableCache')


if __name__ == '__main__':
    unittest.main()

"""Updates: a newer release is found, downloaded and installed only when its
SHA256SUMS.txt is signed with the release key and the download matches it.
A local server on 127.0.0.1 plays GitHub, with a throw-away test key."""
import hashlib, http.server, json, os, plistlib, shutil, subprocess, tempfile, threading, unittest, zipfile
from pathlib import Path
from test_guards import BIN

ROOT = Path(__file__).resolve().parents[1]
SWIFT = shutil.which('swift')
WINDOWS_ENGINE = 'windows-target' in str(BIN)

class Server(http.server.ThreadingHTTPServer):
    files = {}

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_GET(self):
        data = self.server.files.get(self.path)
        if data is None:
            self.send_response(404); self.end_headers(); return
        self.send_response(200); self.send_header('Content-Length', str(len(data))); self.end_headers(); self.wfile.write(data)

def sign(key_dir, path):
    subprocess.run([SWIFT, str(ROOT / 'tools/release-sign.swift'), 'sign', str(path)], env={**os.environ, 'PPTSHOWTOOLS_RELEASE_KEY_DIR': str(key_dir)}, check=True, capture_output=True)

@unittest.skipUnless(SWIFT, 'swift signs the test release')
class Updater(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dir = Path(tempfile.mkdtemp())
        cls.keys, cls.other = cls.dir / 'keys', cls.dir / 'other-keys'
        env = {**os.environ}
        cls.public = subprocess.run([SWIFT, str(ROOT / 'tools/release-sign.swift'), 'keygen'], env={**env, 'PPTSHOWTOOLS_RELEASE_KEY_DIR': str(cls.keys)}, capture_output=True, text=True, check=True).stdout.strip()
        subprocess.run([SWIFT, str(ROOT / 'tools/release-sign.swift'), 'keygen'], env={**env, 'PPTSHOWTOOLS_RELEASE_KEY_DIR': str(cls.other)}, capture_output=True, check=True)
        cls.server = Server(('127.0.0.1', 0), Handler)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.base = f'http://127.0.0.1:{cls.server.server_address[1]}'

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown(); shutil.rmtree(cls.dir, ignore_errors=True)

    def asset(self):
        """The file this engine downloads: a portable zip (Windows engine) or this Mac's disk image."""
        if WINDOWS_ENGINE:
            return 'PPTShowTools-0.10.1-Windows-portable.zip'
        arch = 'AppleSilicon' if os.uname().machine == 'arm64' else 'Intel'
        return f'PPTShowTools-0.10.1-{arch}.dmg'

    def publish(self, payload, sums_for=None, key=None, signed=True, extra_releases=()):
        """Serve a release 0.10.1 whose asset is `payload`; the checksum is of `sums_for` (default: payload)."""
        name = self.asset()
        folder = self.dir / 'release'; shutil.rmtree(folder, ignore_errors=True); folder.mkdir()
        sums = folder / 'SHA256SUMS.txt'
        sums.write_text(f'{hashlib.sha256(sums_for if sums_for is not None else payload).hexdigest()}  {name}\n')
        files = {f'/dl/{name}': payload, '/dl/SHA256SUMS.txt': sums.read_bytes()}
        assets = [{'name': name, 'browser_download_url': f'{self.base}/dl/{name}'}, {'name': 'SHA256SUMS.txt', 'browser_download_url': f'{self.base}/dl/SHA256SUMS.txt'}]
        if signed:
            sign(key or self.keys, sums)
            files['/dl/SHA256SUMS.txt.sig'] = Path(str(sums) + '.sig').read_bytes()
            assets.append({'name': 'SHA256SUMS.txt.sig', 'browser_download_url': f'{self.base}/dl/SHA256SUMS.txt.sig'})
        releases = [{'tag_name': 'v0.10.1', 'draft': False, 'prerelease': True, 'html_url': f'{self.base}/page', 'body': '## New\n**Faster**', 'assets': assets},
                    {'tag_name': 'v0.9.0', 'draft': False, 'prerelease': True, 'html_url': f'{self.base}/old', 'body': '', 'assets': []},
                    {'tag_name': 'v9.0.0', 'draft': True, 'prerelease': False, 'html_url': f'{self.base}/draft', 'body': '', 'assets': []}, *extra_releases]
        Server.files = {**files, '/releases': json.dumps(releases).encode()}

    def run_app(self, *args):
        env = {**os.environ, 'PPTSHOWTOOLS_UPDATE_URL': f'{self.base}/releases', 'PPTSHOWTOOLS_UPDATE_KEY': self.public, 'PPTSHOWTOOLS_VERSION': '0.10.0'}
        return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True, env=env, timeout=120)

    def test_finds_the_newest_published_release(self):
        self.publish(b'x')
        r = self.run_app('--check-update')
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(r.stdout), {'current': '0.10.0', 'latest': '0.10.1', 'newer': True, 'asset': self.asset()})

    def test_downloads_only_a_signed_matching_file(self):
        payload = os.urandom(4096)
        self.publish(payload)
        r = self.run_app('--download-update')
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(Path(r.stdout.strip()).read_bytes(), payload)

    def test_refuses_a_file_that_does_not_match(self):
        self.publish(b'tampered', sums_for=b'original')
        r = self.run_app('--download-update')
        self.assertNotEqual(r.returncode, 0); self.assertIn('does not match the release checksum', r.stderr)

    def test_refuses_a_release_signed_with_another_key(self):
        self.publish(b'x', key=self.other)
        r = self.run_app('--download-update')
        self.assertNotEqual(r.returncode, 0); self.assertIn('signature is not valid', r.stderr)

    def test_refuses_an_unsigned_release(self):
        self.publish(b'x', signed=False)
        r = self.run_app('--download-update')
        self.assertNotEqual(r.returncode, 0); self.assertIn('not signed', r.stderr)

    def test_the_real_key_is_used_outside_the_test_server(self):
        # Pointing the updater elsewhere than 127.0.0.1 ignores the test key and URL.
        env = {**os.environ, 'PPTSHOWTOOLS_UPDATE_URL': 'http://example.com/releases', 'PPTSHOWTOOLS_UPDATE_KEY': self.public, 'PPTSHOWTOOLS_VERSION': '99.0.0'}
        r = subprocess.run([str(BIN), '--check-update'], capture_output=True, text=True, env=env, timeout=120)
        if r.returncode == 0:
            self.assertNotEqual(json.loads(r.stdout)['current'], '99.0.0')

    @unittest.skipIf(WINDOWS_ENGINE, 'macOS replaces its app bundle')
    def test_mac_app_is_replaced_by_the_new_version(self):
        app = BIN.parents[2]
        target_dir = self.dir / 'Applications'; target_dir.mkdir(exist_ok=True)
        target = target_dir / 'PPT Show Tools.app'
        shutil.rmtree(target, ignore_errors=True); subprocess.run(['ditto', str(app), str(target)], check=True)
        new = self.dir / 'dmg-src'; shutil.rmtree(new, ignore_errors=True); new.mkdir()
        subprocess.run(['ditto', str(app), str(new / 'PPT Show Tools.app')], check=True)
        info = new / 'PPT Show Tools.app/Contents/Info.plist'
        plist = plistlib.loads(info.read_bytes()); plist['CFBundleShortVersionString'] = '0.10.1'; info.write_bytes(plistlib.dumps(plist))
        dmg = self.dir / 'new.dmg'
        if dmg.exists(): dmg.unlink()
        subprocess.run(['hdiutil', 'create', '-quiet', '-srcfolder', str(new), '-format', 'UDZO', str(dmg)], check=True)
        r = self.run_app('--install-update', dmg, target, '0.10.1')
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(plistlib.loads((target / 'Contents/Info.plist').read_bytes())['CFBundleShortVersionString'], '0.10.1')
        self.assertTrue(any(p.name.startswith('.PPT Show Tools-old-') for p in target_dir.iterdir()))
        # A disk image with another version is refused.
        r = self.run_app('--install-update', dmg, target, '0.10.2')
        self.assertNotEqual(r.returncode, 0); self.assertIn('expected version', r.stderr)

    @unittest.skipUnless(WINDOWS_ENGINE, 'the portable Windows copy updates its own folder')
    def test_portable_copy_replaces_its_files(self):
        folder = self.dir / 'Portable'; shutil.rmtree(folder, ignore_errors=True); folder.mkdir()
        (folder / 'PPTShowTools.exe').write_bytes(b'old app'); (folder / 'ffmpeg.exe').write_bytes(b'old ffmpeg')
        package = self.dir / 'portable.zip'
        with zipfile.ZipFile(package, 'w') as z:
            z.writestr('PPT Show Tools/PPTShowTools.exe', b'new app'); z.writestr('PPT Show Tools/ffmpeg.exe', b'new ffmpeg')
            z.writestr('PPT Show Tools/../../escape.txt', b'no'); z.writestr('PPT Show Tools/other.dll', b'no')
        r = self.run_app('--install-update', package, folder)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((folder / 'PPTShowTools.exe').read_bytes(), b'new app')
        self.assertEqual((folder / 'ffmpeg.exe').read_bytes(), b'new ffmpeg')
        self.assertEqual((folder / 'PPTShowTools.exe.old').read_bytes(), b'old app')
        self.assertFalse((folder / 'other.dll').exists()); self.assertFalse((self.dir / 'escape.txt').exists())

if __name__ == '__main__':
    unittest.main()

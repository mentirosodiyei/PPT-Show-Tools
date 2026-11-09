"""Images to PowerPoint: pictures or folders become slides, one or several per
slide, never cropped or stretched, with transitions and automatic advance."""
import hashlib, json, os, re, shutil, struct, subprocess, tempfile, unittest, zipfile
from pathlib import Path
from test_guards import BIN
from test_adaptive_resize import png, png_size

SIPS = shutil.which('sips')
P = 'http://schemas.openxmlformats.org/presentationml/2006/main'

def run(*args, **env):
    return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True, env={**os.environ, **env})

def slides(path):
    """Per slide: [(name, x, y, cx, cy)], transition element."""
    with zipfile.ZipFile(path) as z:
        names = sorted((n for n in z.namelist() if re.fullmatch(r'ppt/slides/slide\d+\.xml', n)), key=lambda n: int(re.findall(r'\d+', n)[0]))
        out = []
        for n in names:
            s = z.read(n).decode()
            pics = re.findall(r'<p:pic><p:nvPicPr><p:cNvPr id="\d+" name="([^"]+)"/>.*?<a:off x="(-?\d+)" y="(-?\d+)"/><a:ext cx="(\d+)" cy="(\d+)"/>', s)
            out.append(([(p[0], *map(int, p[1:])) for p in pics], (re.findall(r'<p:transition[^>]*>.*?</p:transition>|<p:transition[^>]*/>', s) or [''])[0]))
        return out

def split_mix(seed):
    state = seed
    while True:
        state = (state + 0x9E3779B97F4A7C15) & (2**64 - 1)
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & (2**64 - 1)
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & (2**64 - 1)
        yield z ^ (z >> 31)

class ImagesToPowerPoint(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp()); self.folder = self.dir / 'Holiday'; self.folder.mkdir()
        # Names sort by number value: img2 before img10.
        for i, (w, h) in zip([1, 2, 10, 3, 4], [(40, 30), (30, 40), (60, 20), (20, 20), (50, 25)]):
            (self.folder / f'img{i}.png').write_bytes(png(w, h))
        (self.folder / '.hidden.png').write_bytes(png(10, 10))
        (self.folder / 'broken.jpg').write_bytes(b'not a picture')
        (self.folder / 'notes.txt').write_text('x')

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def make(self, *args, **env):
        out = self.dir / 'out.pptx'
        for f in [out, Path(str(out) + '.report.json'), Path(str(out) + '.report.log')]:
            if f.exists(): f.unlink()
        r = run('--images-to-ppt', *args, out, self.folder, **env)
        self.assertEqual(r.returncode, 0, r.stderr)
        return out, json.loads(r.stdout)

    def test_one_per_slide_in_name_order_whole_and_unchanged(self):
        out, report = self.make()
        deck = slides(out)
        self.assertEqual([s[0][0][0] for s in deck], ['img1.png', 'img2.png', 'img3.png', 'img4.png', 'img10.png'])
        W, H = 12192000, 6858000
        for pics, transition in deck:
            (name, x, y, cx, cy), = pics
            w, h = png_size((self.folder / name).read_bytes())
            # Fitted whole: touches two opposite edges, keeps its proportion, centred.
            self.assertTrue(cx == W or cy == H, name)
            self.assertAlmostEqual(cx / cy, w / h, delta=0.001 * w / h)
            self.assertLessEqual(abs((W - cx) - 2 * x), 1); self.assertLessEqual(abs((H - cy) - 2 * y), 1)
            self.assertEqual(transition, '')
        with zipfile.ZipFile(out) as z:
            media = {hashlib.sha256(z.read(n)).hexdigest() for n in z.namelist() if n.startswith('ppt/media/') and not n.endswith('/')}
        self.assertEqual(media, {hashlib.sha256((self.folder / f'img{i}.png').read_bytes()).hexdigest() for i in [1, 2, 3, 4, 10]})
        self.assertIn('Skipped (unreadable): broken.jpg.', report['limitations'])

    def test_four_per_slide_grid_without_cropping(self):
        out, _ = self.make('--layout', 'four', '--size', '1920x1080')
        deck = slides(out)
        self.assertEqual([len(p) for p, _ in deck], [4, 1])
        W, H = 1920 * 9525, 1080 * 9525
        m = round(0.03 * 1080 * 9525)
        for name, x, y, cx, cy in deck[0][0]:
            self.assertTrue(m - 1 <= x and x + cx <= W - m + 1 and m - 1 <= y and y + cy <= H - m + 1, name)
            w, h = png_size((self.folder / name).read_bytes())
            self.assertAlmostEqual(cx / cy, w / h, delta=0.002 * w / h)
        # Four cells, one picture in each quadrant.
        quadrants = {(x + cx / 2 > W / 2, y + cy / 2 > H / 2) for _, x, y, cx, cy in deck[0][0]}
        self.assertEqual(len(quadrants), 4)

    def test_random_layout_follows_the_seed_on_every_engine(self):
        out, _ = self.make('--layout', 'random', PPTSHOWTOOLS_RANDOM_SEED='42')
        counts, left, rnd = [], 5, split_mix(42)
        while left:
            n = 1 + next(rnd) % min(4, left); counts.append(n); left -= n
        self.assertEqual([len(p) for p, _ in slides(out)], counts)

    def test_transition_and_automatic_advance(self):
        out, report = self.make('--transition', 'push', '--advance', '4')
        for _, transition in slides(out):
            self.assertEqual(transition, '<p:transition spd="med" advTm="4000"><p:push dir="l"/></p:transition>')
        self.assertIn('Transition on every slide', report['checks'])
        out, _ = self.make('--advance', '2.5')
        self.assertEqual(slides(out)[0][1], '<p:transition advTm="2500"/>')

    def background(self, out):
        with zipfile.ZipFile(out) as z:
            return {re.search(r'<p:bg><p:bgPr><a:solidFill><a:srgbClr val="([0-9A-F]{6})"/>', z.read(n).decode()).group(1)
                    for n in z.namelist() if re.fullmatch(r'ppt/slides/slide\d+\.xml', n)}

    def test_several_per_slide_use_the_chosen_background(self):
        for layout in ['four', 'random']:
            out, report = self.make('--layout', layout, '--background', '#1f3864')
            self.assertEqual(self.background(out), {'1F3864'}, layout)
            self.assertIn('the chosen background colour (#1F3864).', report['limitations'][0])
        # One per slide stays on black.
        out, _ = self.make('--background', '1F3864')
        self.assertEqual(self.background(out), {'000000'})
        r = run('--images-to-ppt', '--layout', 'four', '--background', 'blue', self.dir / 'bad.pptx', self.folder)
        self.assertEqual(r.returncode, 1); self.assertIn('six hexadecimal digits', r.stderr)

    def test_bad_options_and_existing_output_are_refused(self):
        out = self.dir / 'out.pptx'
        for args in [('--layout', 'six'), ('--transition', 'spin'), ('--advance', '0')]:
            r = run('--images-to-ppt', *args, out, self.folder)
            self.assertEqual(r.returncode, 1, args); self.assertFalse(out.exists())
        out.write_bytes(b'keep')
        r = run('--images-to-ppt', out, self.folder)
        self.assertEqual(r.returncode, 1); self.assertEqual(out.read_bytes(), b'keep')
        empty = self.dir / 'empty'; empty.mkdir()
        r = run('--images-to-ppt', self.dir / 'none.pptx', empty)
        self.assertEqual(r.returncode, 1); self.assertIn('No images found', r.stderr)

    def test_sideways_photo_is_turned_upright(self):
        if not SIPS: self.skipTest('sips makes the test JPEG')
        source = self.dir / 'wide.png'; source.write_bytes(png(60, 20))
        jpeg = self.dir / 'plain.jpg'
        subprocess.run([SIPS, '-s', 'format', 'jpeg', str(source), '--out', str(jpeg)], capture_output=True, check=True)
        data = jpeg.read_bytes()
        # Keep the JPEG's segments except its own EXIF block, and add one saying orientation 6.
        segments, i = [], 2
        while data[i] == 0xFF and data[i + 1] != 0xDA:
            n = int.from_bytes(data[i + 2:i + 4], 'big'); segments.append(data[i:i + 2 + n]); i += 2 + n
        tiff = b'MM\x00\x2a' + struct.pack('>I', 8) + struct.pack('>H', 1) + struct.pack('>HHII', 0x0112, 3, 1, 6 << 16) + struct.pack('>I', 0)
        app1 = b'Exif\x00\x00' + tiff
        exif = b'\xff\xe1' + struct.pack('>H', len(app1) + 2) + app1
        kept = [seg for seg in segments if seg[1] != 0xE1]
        folder = self.dir / 'photos'; folder.mkdir()
        (folder / 'turned.jpg').write_bytes(data[:2] + kept[0] + exif + b''.join(kept[1:]) + data[i:])
        if SIPS: subprocess.run([SIPS, '-s', 'format', 'heic', str(jpeg), '--out', str(folder / 'phone.heic')], capture_output=True)
        out = self.dir / 'photos.pptx'
        r = run('--images-to-ppt', out, folder)
        self.assertEqual(r.returncode, 0, r.stderr)
        report = json.loads(r.stdout)
        notes = [w for p in report['pages'] for w in p['warnings']]
        self.assertIn('turned.jpg: turned upright.', notes)
        turned = next(p for p, _ in slides(out) if p[0][0] == 'turned.jpg')[0]
        self.assertLess(turned[3], turned[4])  # now taller than wide
        if (folder / 'phone.heic').exists() and not any('phone.heic' in s for s in report['limitations']):
            self.assertIn('phone.heic: converted from HEIC to JPEG.', notes)

if __name__ == '__main__':
    unittest.main()

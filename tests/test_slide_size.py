"""Output slide size: automatic detection and sizes forced with --size (Settings)."""
import json, subprocess, tempfile, unittest, zipfile
from pathlib import Path
import xml.etree.ElementTree as E
from test_guards import fixture, BIN

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
PX = 9525  # EMU per pixel at 96 per inch, as PowerPoint measures them

def deck(path, width, height, shape=None, motion=False):
    """Fixture deck of the given EMU size with one rectangle (x, y, cx, cy)."""
    fixture(path, width, height)
    x, y, cx, cy = shape or (0, 0, width, height)
    timing = ''
    if motion:
        timing = '<p:timing><p:tnLst><p:par><p:cTn id="1"><p:childTnLst><p:animMotion origin="layout" path="M 0 0 L 0.25 0.5 E"><p:cBhvr><p:cTn id="2" dur="1000"/><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cBhvr></p:animMotion></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>'
    slide = f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Box"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld>{timing}</p:sld>'
    with zipfile.ZipFile(path) as z:
        parts = {n: z.read(n) for n in z.namelist()}
    parts['ppt/slides/slide1.xml'] = slide.encode()
    with zipfile.ZipFile(path, 'w') as z:
        for n, v in parts.items():
            z.writestr(n, v)

def size_of(path):
    with zipfile.ZipFile(path) as z:
        s = E.fromstring(z.read('ppt/presentation.xml')).find('{' + P + '}sldSz')
    return int(s.get('cx')), int(s.get('cy'))

def box(path, part):
    with zipfile.ZipFile(path) as z:
        root = E.fromstring(z.read(part))
    off, ext = root.find('.//{' + A + '}off'), root.find('.//{' + A + '}ext')
    return int(off.get('x')), int(off.get('y')), int(ext.get('cx')), int(ext.get('cy'))

def run(*args):
    return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True)

class SlideSize(unittest.TestCase):
    def test_automatic_keeps_a_size_shared_by_every_deck(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            deck(a, 4800 * PX, 1344 * PX); deck(b, 4800 * PX, 1344 * PX)
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(size_of(o), (4800 * PX, 1344 * PX))
            self.assertEqual(box(o, 'pptMerger/source1/ppt/slides/slide1.xml'), (0, 0, 4800 * PX, 1344 * PX))

    def test_automatic_same_proportion_uses_the_largest(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'big.pptx', d / 'small.pptx', d / 'o.pptx'
            deck(a, 4800 * PX, 1344 * PX); deck(b, 2400 * PX, 672 * PX, (100, 200, 2400 * PX, 672 * PX))
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(size_of(o), (4800 * PX, 1344 * PX))
            self.assertEqual(box(o, 'pptMerger/source1/ppt/slides/slide1.xml'), (200, 400, 4800 * PX, 1344 * PX))
            report = json.loads(Path(str(o) + '.report.json').read_text())
            self.assertTrue(all(not v['issues'] for v in report['validation']))

    def test_fixed_size_adapts_full_slide_shapes(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            deck(a, 12192000, 6858000, motion=True); deck(b, 12192000, 6858000)
            r = run('--merge', '--size', '4800x1344', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            tw, th = 4800 * PX, 1344 * PX
            self.assertEqual(size_of(o), (tw, th))
            # A shape covering the slide keeps covering it, with no empty bands.
            self.assertEqual(box(o, 'ppt/slides/slide1.xml'), (0, 0, tw, th))
            report = json.loads(Path(str(o) + '.report.json').read_text())
            self.assertTrue(all(not v['issues'] for v in report['validation']), report['validation'])

    def test_mixed_proportions_need_no_question(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'wide.pptx', d / 'old.pptx', d / 'o.pptx'
            deck(a, 12192000, 6858000); deck(b, 9144000, 6858000)
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(size_of(o), (12192000, 6858000))
            o.unlink(); Path(str(o) + '.report.json').unlink(); Path(str(o) + '.report.txt').unlink()
            r = run('--merge', '--size', '1920x1080', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(size_of(o), (1920 * PX, 1080 * PX))

    def test_resize_to_a_fixed_size(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, o = d / 'old.pptx', d / 'o.pptx'
            deck(a, 9144000, 6858000)
            r = run('--resize', '--size', '1920x1080', o, a)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(size_of(o), (18288000, 10287000))
            self.assertEqual(box(o, 'ppt/slides/slide1.xml'), (0, 0, 18288000, 10287000))
            again = run('--resize', '--size', '1920x1080', d / 'o2.pptx', o)
            self.assertNotEqual(again.returncode, 0)
            self.assertIn('already', again.stderr)

    def test_resize_automatic_widens_to_16_9(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, o = d / 'old.pptx', d / 'o.pptx'
            deck(a, 9144000, 6858000)
            r = run('--resize', o, a)
            self.assertEqual(r.returncode, 0, r.stderr)
            w, h = size_of(o)
            self.assertEqual(w * 9, h * 16)
            self.assertEqual(box(o, 'ppt/slides/slide1.xml'), (0, 0, w, h))

    def test_sizes_outside_powerpoint_limits_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            for size in ['50x50', '6000x1000', 'big']:
                r = run('--merge', '--size', size, o, a, b)
                self.assertEqual(r.returncode, 1, size)
                self.assertFalse(o.exists())

if __name__ == '__main__':
    unittest.main()

"""Adaptive resizing: the layout follows the new size without stretching anything
and without leaving empty bands (Merge, Resize slides and PDF to PowerPoint)."""
import base64, subprocess, tempfile, unittest, zipfile
from pathlib import Path
import xml.etree.ElementTree as E
from test_guards import BIN

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
K = 'http://schemas.openxmlformats.org/package/2006/relationships'
NS = {'p': P, 'a': A}
PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
W, H = 9144000, 6858000  # 4:3

def xfrm(x, y, cx, cy):
    return f'<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'
def pic(i, name, frame):
    return f'<p:pic><p:nvPicPr><p:cNvPr id="{i}" name="{name}"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="img"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{frame}</p:spPr></p:pic>'
def shape(i, name, frame, text=None, geom='rect'):
    body = f'<p:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="2400"/><a:t>{text}</a:t></a:r></a:p></p:txBody>' if text else ''
    return f'<p:sp><p:nvSpPr><p:cNvPr id="{i}" name="{name}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{frame}<a:prstGeom prst="{geom}"><a:avLst/></a:prstGeom></p:spPr>{body}</p:sp>'
def group(i, frame_children):
    return f'<p:grpSp><p:nvGrpSpPr><p:cNvPr id="{i}" name="Group"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="3000000" y="3000000"/><a:ext cx="1000000" cy="500000"/><a:chOff x="0" y="0"/><a:chExt cx="1000000" cy="500000"/></a:xfrm></p:grpSpPr>{frame_children}</p:grpSp>'

def build(path, body, width=W, height=H, background=False):
    """One-slide deck whose master may have a picture background."""
    grp = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>'
    bg = '<p:bg><p:bgPr><a:blipFill><a:blip r:embed="img"/><a:stretch><a:fillRect/></a:stretch></a:blipFill><a:effectLst/></p:bgPr></p:bg>' if background else ''
    rels = lambda items: f'<Relationships xmlns="{K}">' + ''.join(f'<Relationship Id="{i}" Type="{R}/{t}" Target="{x}"/>' for i, t, x in items) + '</Relationships>'
    parts = {
        '[Content_Types].xml': '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/></Types>',
        '_rels/.rels': rels([('r1', 'officeDocument', 'ppt/presentation.xml')]),
        'ppt/presentation.xml': f'<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="m"/></p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="s"/></p:sldIdLst><p:sldSz cx="{width}" cy="{height}"/></p:presentation>',
        'ppt/_rels/presentation.xml.rels': rels([('m', 'slideMaster', 'slideMasters/slideMaster1.xml'), ('s', 'slide', 'slides/slide1.xml')]),
        'ppt/slideMasters/slideMaster1.xml': f'<p:sldMaster xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld>{bg}<p:spTree>{grp}</p:spTree></p:cSld><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="l"/></p:sldLayoutIdLst></p:sldMaster>',
        'ppt/slideMasters/_rels/slideMaster1.xml.rels': rels([('l', 'slideLayout', '../slideLayouts/slideLayout1.xml'), ('img', 'image', '../media/img.png')]),
        'ppt/slideLayouts/slideLayout1.xml': f'<p:sldLayout xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>{grp}</p:spTree></p:cSld></p:sldLayout>',
        'ppt/slideLayouts/_rels/slideLayout1.xml.rels': rels([('m', 'slideMaster', '../slideMasters/slideMaster1.xml')]),
        'ppt/slides/slide1.xml': f'<p:sld xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:spTree>{grp}{body}</p:spTree></p:cSld></p:sld>',
        'ppt/slides/_rels/slide1.xml.rels': rels([('l', 'slideLayout', '../slideLayouts/slideLayout1.xml'), ('img', 'image', '../media/img.png')]),
    }
    with zipfile.ZipFile(path, 'w') as z:
        for n, v in parts.items():
            z.writestr(n, v)
        z.writestr('ppt/media/img.png', PNG)

def run(*args):
    return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True)

def frames(path, part='ppt/slides/slide1.xml'):
    """name -> (x, y, cx, cy) of every top-level object."""
    with zipfile.ZipFile(path) as z:
        root = E.fromstring(z.read(part))
    out = {}
    for e in root.find('.//p:spTree', NS):
        name = e.find('.//p:cNvPr', NS)
        holder = e.find('p:spPr/a:xfrm', NS) or e.find('p:grpSpPr/a:xfrm', NS)
        if name is not None and holder is not None:
            off, ext = holder.find('a:off', NS), holder.find('a:ext', NS)
            out[name.get('name')] = (int(off.get('x')), int(off.get('y')), int(ext.get('cx')), int(ext.get('cy')))
    return out, root

class AdaptiveResize(unittest.TestCase):
    def resize(self, body, size=None, background=False):
        d = tempfile.mkdtemp()
        src, out = Path(d) / 'in.pptx', Path(d) / 'out.pptx'
        build(src, body, background=background)
        r = run('--resize', *(['--size', size] if size else []), out, src)
        self.assertEqual(r.returncode, 0, r.stderr)
        return out

    def test_4_3_to_16_9_keeps_every_object_undistorted_and_in_place(self):
        body = (shape(2, 'Title', xfrm(457200, 274638, 8229600, 1143000), text='Quarterly review')
                + pic(3, 'Logo', xfrm(8001000, 5943600, 685800, 685800))
                + pic(4, 'Photo', xfrm(3200400, 2286000, 2743200, 2743200))
                + shape(5, 'Bar', xfrm(0, 6400800, W, 457200))
                + shape(6, 'Dot', xfrm(304800, 2743200, 914400, 914400), geom='ellipse'))
        f, root = frames(self.resize(body))
        # Text box: follows the new proportion (width × 4/3), same height.
        self.assertEqual(f['Title'], (609600, 274638, 10972800, 1143000))
        # Logo in the bottom-right corner: same size, same margins to the corner.
        self.assertEqual(f['Logo'], (12192000 - 457200 - 685800, 5943600, 685800, 685800))
        # Photo in the middle: same size, still centred.
        self.assertEqual(f['Photo'], (6096000 - 1371600, 2286000, 2743200, 2743200))
        # Full-width bar: still spans the slide.
        self.assertEqual(f['Bar'], (0, 6400800, 12192000, 457200))
        # Circle near the left edge: still a circle, same margin.
        self.assertEqual(f['Dot'], (304800, 2743200, 914400, 914400))
        self.assertEqual(root.find('.//a:rPr', NS).get('sz'), '2400')
        self.assertIsNone(root.find('.//a:srcRect', NS))

    def test_full_slide_picture_is_cropped_not_stretched(self):
        f, root = frames(self.resize(pic(2, 'Cover', xfrm(0, 0, W, H))))
        self.assertEqual(f['Cover'], (0, 0, 12192000, 6858000))
        rect = root.find('.//p:blipFill/a:srcRect', NS)
        # 4:3 picture in a 16:9 box: show the middle 75 % of its height.
        self.assertEqual((rect.get('t'), rect.get('b')), ('12500', '12500'))

    def test_picture_background_overflows_instead_of_stretching(self):
        out = self.resize(shape(2, 'Note', xfrm(457200, 457200, 3000000, 600000), text='x'), background=True)
        with zipfile.ZipFile(out) as z:
            master = E.fromstring(z.read('ppt/slideMasters/slideMaster1.xml'))
        rect = master.find('.//p:bgPr/a:blipFill/a:stretch/a:fillRect', NS)
        self.assertEqual((rect.get('t'), rect.get('b')), ('-16667', '-16667'))

    def test_ultra_wide_size_scales_text_and_keeps_groups_intact(self):
        body = shape(2, 'Body', xfrm(457200, 1600200, 4114800, 3200400), text='Revenue up 12%') + group(3, shape(4, 'Child', xfrm(0, 0, 1000000, 500000)))
        f, root = frames(self.resize(body, size='4800x1344'))
        s = min(4800 * 9525 / W, 1344 * 9525 / H)
        self.assertEqual(root.find('.//a:rPr', NS).get('sz'), str(round(2400 * s)))
        # The group keeps its proportion and its content scales with it.
        gx, gy, gw, gh = f['Group']
        self.assertEqual((gw, gh), (round(1000000 * s), round(500000 * s)))
        child = root.find('.//p:grpSp/p:sp/p:spPr/a:xfrm/a:ext', NS)
        self.assertEqual((int(child.get('cx')), int(child.get('cy'))), (round(1000000 * s), round(500000 * s)))
        # The body text box spans the same share of the new width.
        self.assertEqual(f['Body'][2], round(4114800 * 4800 * 9525 / W))

    def test_same_proportion_is_plain_proportional_scaling(self):
        body = pic(2, 'Logo', xfrm(8001000, 5943600, 685800, 685800))
        f, _ = frames(self.resize(body, size='2048x1536'))  # 4:3, larger
        s = 2048 * 9525 / W
        self.assertEqual(f['Logo'], (round(8001000 * s), round(5943600 * s), round(685800 * s), round(685800 * s)))

class PdfSize(unittest.TestCase):
    def test_pdf_slides_take_the_settings_size(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, o = d / 'page.pdf', d / 'o.pptx'
            objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>', b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>']
            pdf = b'%PDF-1.4\n'; offsets = []
            for i, obj in enumerate(objects, 1):
                offsets.append(len(pdf)); pdf += f'{i} 0 obj\n'.encode() + obj + b'\nendobj\n'
            xref = len(pdf); pdf += f'xref\n0 {len(objects)+1}\n0000000000 65535 f \n'.encode() + b''.join(f'{x:010} 00000 n \n'.encode() for x in offsets)
            pdf += f'trailer << /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF'.encode(); a.write_bytes(pdf)
            r = run('--pdf-to-ppt', '--size', '1920x1080', o, a)
            if 'Windows renderer' in r.stderr:
                self.skipTest('PDF rendering needs the Windows renderer in this build')
            self.assertEqual(r.returncode, 0, r.stderr)
            with zipfile.ZipFile(o) as z:
                size = E.fromstring(z.read('ppt/presentation.xml')).find('p:sldSz', NS)
            self.assertEqual((size.get('cx'), size.get('cy')), ('18288000', '10287000'))

if __name__ == '__main__':
    unittest.main()

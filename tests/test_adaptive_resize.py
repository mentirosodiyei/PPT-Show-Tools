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
def png(width, height):
    """RGB test picture: red on the left half, blue on the right, green bottom row."""
    import struct, zlib
    rows = b''
    for y in range(height):
        rows += b'\0' + b''.join(bytes((0, 200, 0) if y == height - 1 else ((220, 0, 0) if x < width // 2 else (0, 0, 220))) for x in range(width))
    chunk = lambda kind, data: struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b'')
PNG = png(40, 30)

def png_size(data):
    import struct
    return struct.unpack('>II', data[16:24])
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

    def test_4_3_to_16_9_keeps_the_design_and_centres_it(self):
        body = (shape(2, 'Title', xfrm(457200, 274638, 8229600, 1143000), text='Quarterly review')
                + pic(3, 'Logo', xfrm(8001000, 5943600, 685800, 685800))
                + pic(4, 'Photo', xfrm(3200400, 2286000, 2743200, 2743200))
                + shape(5, 'Bar', xfrm(0, 6400800, W, 457200))
                + shape(6, 'Dot', xfrm(304800, 2743200, 914400, 914400), geom='ellipse'))
        f, root = frames(self.resize(body))
        # Nothing moves relative to anything else: the 4:3 page is centred in
        # the 16:9 one (1524000 EMU to the right) and nothing changes size.
        dx = 1524000
        self.assertEqual(f['Title'], (457200 + dx, 274638, 8229600, 1143000))
        self.assertEqual(f['Logo'], (8001000 + dx, 5943600, 685800, 685800))
        self.assertEqual(f['Photo'], (3200400 + dx, 2286000, 2743200, 2743200))
        self.assertEqual(f['Bar'], (dx, 6400800, W, 457200))
        self.assertEqual(f['Dot'], (304800 + dx, 2743200, 914400, 914400))
        self.assertEqual(root.find('.//a:rPr', NS).get('sz'), '2400')
        self.assertIsNone(root.find('.//a:srcRect', NS))

    def padded_picture(self, out, part, blip_owner):
        """The image a picture fill now uses, and its size in pixels."""
        with zipfile.ZipFile(out) as z:
            root = E.fromstring(z.read(part))
            embed = root.find(blip_owner + '/a:blip', NS).get('{' + R + '}embed')
            folder, name = part.rsplit('/', 1)
            rels = E.fromstring(z.read(f'{folder}/_rels/{name}.rels'))
            target = next(r.get('Target') for r in rels if r.get('Id') == embed)
            data = z.read(target.lstrip('/'))
            types = z.read('[Content_Types].xml').decode()
        return root, target, png_size(data), data, types

    def test_full_slide_picture_is_shown_whole_never_cropped(self):
        out = self.resize(pic(2, 'Cover', xfrm(0, 0, W, H)))
        f, root = frames(out)
        self.assertEqual(f['Cover'], (0, 0, 12192000, 6858000))
        self.assertIsNone(root.find('.//a:srcRect', NS))
        _, target, size, _, types = self.padded_picture(out, 'ppt/slides/slide1.xml', './/p:pic/p:blipFill')
        # 4:3 picture in a 16:9 frame: the whole 40 × 30 picture with side margins.
        self.assertRegex(target, r'^/ppt/media/pst-pad-[0-9a-f]{8}\.png$')
        self.assertEqual(size, (53, 30))
        self.assertIn('Extension="png"', types)

    def test_margins_take_the_colours_of_the_picture_edges(self):
        import zlib
        out = self.resize(pic(2, 'Cover', xfrm(0, 0, W, H)), size='3427x534')
        _, _, (w, h), data, _ = self.padded_picture(out, 'ppt/slides/slide1.xml', './/p:pic/p:blipFill')
        self.assertEqual((w, h), (round(40 * 3427 / 534 * 30 / 40), 30))
        # Decode the PNG rows (RGBA or RGB, no filtering assumptions: use the decoder of the engine output).
        idat = b''; i = 8
        while i < len(data):
            n = int.from_bytes(data[i:i + 4], 'big'); kind = data[i + 4:i + 8]
            if kind == b'IDAT': idat += data[i + 8:i + 8 + n]
            i += 12 + n
        bpp = 4 if data[25] == 6 else 3
        raw = zlib.decompress(idat); stride = w * bpp + 1
        rows, prev = [], bytearray(w * bpp)
        for y in range(h):
            f, line = raw[y * stride], bytearray(raw[y * stride + 1:(y + 1) * stride])
            for x in range(len(line)):
                a = line[x - bpp] if x >= bpp else 0; b = prev[x]; c = prev[x - bpp] if x >= bpp else 0
                if f == 1: line[x] = (line[x] + a) & 255
                elif f == 2: line[x] = (line[x] + b) & 255
                elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
                elif f == 4:
                    p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                    line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
            rows.append(bytes(line)); prev = line
        pixel = lambda x, y: tuple(rows[y][x * bpp:x * bpp + 3])
        left, right = pixel(0, h // 3), pixel(w - 1, h // 3)
        self.assertGreater(left[0], 150); self.assertLess(left[2], 80)    # red side continues red
        self.assertGreater(right[2], 150); self.assertLess(right[0], 80)  # blue side continues blue

    def test_off_centre_full_slide_picture_reaches_both_edges(self):
        # A photo covering 95 % of the width from the left edge: it keeps its place in
        # the centred page and is extended, unequally, to both new edges.
        width = round(0.95 * W)
        out = self.resize(pic(2, 'Cover', xfrm(0, 0, width, H)))
        f, root = frames(out)
        self.assertEqual(f['Cover'], (0, 0, 12192000, 6858000))
        _, _, size, _, _ = self.padded_picture(out, 'ppt/slides/slide1.xml', './/p:pic/p:blipFill')
        self.assertEqual(size, (round(40 * 12192000 / width), 30))

    def test_picture_background_is_shown_whole_never_cropped(self):
        out = self.resize(shape(2, 'Note', xfrm(457200, 457200, 3000000, 600000), text='x'), background=True)
        master, target, size, _, _ = self.padded_picture(out, 'ppt/slideMasters/slideMaster1.xml', './/p:bgPr/a:blipFill')
        rect = master.find('.//p:bgPr/a:blipFill/a:stretch/a:fillRect', NS)
        self.assertFalse(rect is not None and any(rect.get(k) for k in 'ltrb'))
        self.assertEqual(size, (53, 30))

    def test_picture_that_cannot_be_padded_keeps_its_proportion(self):
        # An existing crop cannot be padded: the picture keeps its shape instead of being cropped more.
        body = pic(2, 'Cropped', xfrm(0, 0, W, H)).replace('<a:stretch>', '<a:srcRect l="1000"/><a:stretch>')
        f, root = frames(self.resize(body))
        x, y, cx, cy = f['Cropped']
        self.assertEqual((cx, cy), (W, H))
        self.assertEqual(x, (12192000 - W) // 2)
        self.assertEqual(root.find('.//a:srcRect', NS).get('l'), '1000')

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
        # The text box keeps its proportion, scaled like everything else.
        self.assertEqual(f['Body'][2:], (round(4114800 * s), round(3200400 * s)))
        self.assertEqual(f['Body'][0], round(457200 * s + (4800 * 9525 - W * s) / 2))

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

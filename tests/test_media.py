"""Extract media: pictures, sounds and videos of chosen slides, copied byte for
byte into a new folder; poster frames, template pictures and links are left out."""
import hashlib, json, os, shutil, subprocess, tempfile, unittest, zipfile
from pathlib import Path
from test_guards import BIN
from test_adaptive_resize import png

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
K = 'http://schemas.openxmlformats.org/package/2006/relationships'
MEDIA = 'http://schemas.microsoft.com/office/2007/relationships/media'
GRP = '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>'

def rels(items):
    external = ' TargetMode="External"'
    return f'<Relationships xmlns="{K}">' + ''.join(f'<Relationship Id="{i}" Type="{t}" Target="{x}"{external if x.startswith("http") else ""}/>' for i, t, x in items) + '</Relationships>'

def pic(i, embed, nv=''):
    return f'<p:pic><p:nvPicPr><p:cNvPr id="{i}" name="P{i}"/><p:cNvPicPr/><p:nvPr>{nv}</p:nvPr></p:nvPicPr><p:blipFill><a:blip r:embed="{embed}"/></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/></a:xfrm></p:spPr></p:pic>'

def slide(body):
    return f'<p:sld xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:spTree>{GRP}{body}</p:spTree></p:cSld></p:sld>'

FILES = {'ppt/media/image1.png': png(40, 30), 'ppt/media/image2.jpg': b'\xff\xd8\xff\xe0jpeg-bytes', 'ppt/media/poster.png': png(16, 9),
         'ppt/media/icon.png': png(8, 8), 'ppt/media/logo.png': png(20, 20), 'ppt/media/media1.mp4': b'video-bytes' * 50, 'ppt/media/media2.mp3': b'audio-bytes' * 30}

def build(path):
    video = '<a:videoFile r:link="rV"/><p:extLst><p:ext uri="{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"><p14:media xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" r:embed="rM"/></p:ext></p:extLst>'
    audio = '<a:audioFile r:link="rA"/><p:extLst><p:ext uri="{DAA4B4D4-6D71-4841-9C94-3DE7FCFB9230}"><p14:media xmlns:p14="http://schemas.microsoft.com/office/powerpoint/2010/main" r:embed="rM"/></p:ext></p:extLst>'
    ct = '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpg" ContentType="image/jpeg"/><Default Extension="mp4" ContentType="video/mp4"/><Default Extension="mp3" ContentType="audio/mpeg"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>' + ''.join(f'<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>' for i in (1, 2, 3)) + '<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/></Types>'
    layout = ('l', f'{R}/slideLayout', '../slideLayouts/slideLayout1.xml')
    parts = {
        '[Content_Types].xml': ct,
        '_rels/.rels': rels([('r1', f'{R}/officeDocument', 'ppt/presentation.xml')]),
        'ppt/presentation.xml': f'<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="m"/></p:sldMasterIdLst><p:sldIdLst>' + ''.join(f'<p:sldId id="{255 + i}" r:id="s{i}"/>' for i in (1, 2, 3)) + '</p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>',
        'ppt/_rels/presentation.xml.rels': rels([('m', f'{R}/slideMaster', 'slideMasters/slideMaster1.xml')] + [(f's{i}', f'{R}/slide', f'slides/slide{i}.xml') for i in (1, 2, 3)]),
        'ppt/slideMasters/slideMaster1.xml': f'<p:sldMaster xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:spTree>{GRP}</p:spTree></p:cSld><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="l"/></p:sldLayoutIdLst></p:sldMaster>',
        'ppt/slideMasters/_rels/slideMaster1.xml.rels': rels([layout[:2] + ('../slideLayouts/slideLayout1.xml',)]),
        'ppt/slideLayouts/slideLayout1.xml': f'<p:sldLayout xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:spTree>{GRP}{pic(2, "logo")}</p:spTree></p:cSld></p:sldLayout>',
        'ppt/slideLayouts/_rels/slideLayout1.xml.rels': rels([('m', f'{R}/slideMaster', '../slideMasters/slideMaster1.xml'), ('logo', f'{R}/image', '../media/logo.png')]),
        'ppt/slides/slide1.xml': slide(pic(2, 'i1') + pic(3, 'rP', video)),
        'ppt/slides/_rels/slide1.xml.rels': rels([layout, ('i1', f'{R}/image', '../media/image1.png'), ('rP', f'{R}/image', '../media/poster.png'), ('rV', f'{R}/video', '../media/media1.mp4'), ('rM', MEDIA, '../media/media1.mp4')]),
        'ppt/slides/slide2.xml': slide(pic(2, 'rI', audio) + pic(3, 'i1') + pic(4, 'i2')),
        'ppt/slides/_rels/slide2.xml.rels': rels([layout, ('rI', f'{R}/image', '../media/icon.png'), ('rA', f'{R}/audio', '../media/media2.mp3'), ('rM', MEDIA, '../media/media2.mp3'), ('i1', f'{R}/image', '../media/image1.png'), ('i2', f'{R}/image', '../media/image2.jpg')]),
        'ppt/slides/slide3.xml': slide(pic(2, 'web')),
        'ppt/slides/_rels/slide3.xml.rels': rels([layout, ('web', f'{R}/image', 'https://example.com/photo.jpg')]),
    }
    with zipfile.ZipFile(path, 'w') as z:
        for n, v in {**parts, **FILES}.items():
            z.writestr(n, v)

def run(*args):
    return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True)

class ExtractMedia(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp()); self.deck = self.dir / 'Talk.pptx'; build(self.deck)
        self.out = self.dir / 'out'; self.out.mkdir()

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)

    def extract(self, *args):
        r = run('--extract-media', *args, self.out, self.deck)
        self.assertEqual(r.returncode, 0, r.stderr)
        report = json.loads(r.stdout)
        folder = Path(report['output'])
        return folder, sorted(p.name for p in folder.iterdir()), report

    def same(self, folder, name, part):
        self.assertEqual(hashlib.sha256((folder / name).read_bytes()).digest(), hashlib.sha256(FILES[part]).digest(), name)

    def test_everything_from_every_slide(self):
        folder, names, report = self.extract()
        self.assertEqual(folder.name, 'Talk media')
        self.assertEqual(names, ['slide-1-image-1.png', 'slide-1-video-1.mp4', 'slide-2-audio-1.mp3', 'slide-2-image-1.jpg'])
        self.same(folder, 'slide-1-image-1.png', 'ppt/media/image1.png')
        self.same(folder, 'slide-1-video-1.mp4', 'ppt/media/media1.mp4')
        self.same(folder, 'slide-2-image-1.jpg', 'ppt/media/image2.jpg')
        text = ' '.join(report['limitations'])
        self.assertIn('slide-1-image-1.png (slides 1, 2)', text)    # used twice, extracted once
        self.assertIn('https://example.com/photo.jpg', text)        # linked, not extracted
        self.assertNotIn('logo', ' '.join(names))                   # template picture
        self.assertNotIn('poster', ' '.join(names))

    def test_only_some_kinds_and_slides(self):
        _, names, report = self.extract('--kinds', 'images', '--slides', '2')
        self.assertEqual(names, ['slide-2-image-1.png', 'slide-2-image-2.jpg'])
        self.assertIn('2 images from slides 2', report['checks'][0])
        _, names, _ = self.extract('--kinds', 'audio,video', '--slides', '1-2')
        self.assertEqual(names, ['slide-1-video-1.mp4', 'slide-2-audio-1.mp3'])

    def test_a_second_extraction_goes_to_a_new_folder(self):
        first, _, _ = self.extract('--kinds', 'video')
        second, _, _ = self.extract('--kinds', 'video')
        self.assertEqual((first.name, second.name), ('Talk media', 'Talk media 2'))

    def test_bad_requests_create_nothing(self):
        for args, message in [(('--slides', '5'), 'slide 5 does not exist'), (('--slides', '2-x'), 'numbers or ranges'),
                              (('--kinds', 'text'), '--kinds'), (('--kinds', 'images', '--slides', '3'), 'only linked')]:
            r = run('--extract-media', *args, self.out, self.deck)
            self.assertEqual(r.returncode, 1, args); self.assertIn(message, r.stderr)
        # Only the error logs the test mode writes next to the output; no media folder.
        self.assertEqual([p for p in self.out.iterdir() if not p.name.startswith('PPTShowTools-ERROR-')], [])

if __name__ == '__main__':
    unittest.main()

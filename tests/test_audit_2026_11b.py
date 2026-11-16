"""Second external audit, November 2026: one regression test per finding that
can be checked from the command line, on both engines."""
import json, os, re, shutil, struct, subprocess, tempfile, time, unittest, zipfile, zlib
from pathlib import Path
from test_guards import fixture, BIN
from test_security import parts, write, run, WINDOWS_ENGINE
from test_conversion_tools import notes_fixture
from test_reports import run as run_installed

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
K = 'http://schemas.openxmlformats.org/package/2006/relationships'
IMAGE = f'{R}/image'
FFMPEG = shutil.which('ffmpeg') or next((p for p in ['/opt/homebrew/bin/ffmpeg', '/usr/local/bin/ffmpeg'] if os.path.exists(p)), None)
BACKDROP = 'PPT Show Tools blurred background'

def rels(*items):
    return f'<Relationships xmlns="{K}">' + ''.join(f'<Relationship Id="{i}" Type="{t}" Target="{x}"/>' for i, t, x in items) + '</Relationships>'

def png(width, height, colour=(200, 80, 40)):
    """A solid RGB PNG, compressed row by row so very large pictures stay cheap to make."""
    def chunk(kind, data): return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)
    row = b'\x00' + bytes(colour) * width
    packer, body = zlib.compressobj(9), []
    for _ in range(height): body.append(packer.compress(row))
    body.append(packer.flush())
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)) + chunk(b'IDAT', b''.join(body)) + chunk(b'IEND', b'')

def png_size(data): return struct.unpack('>II', data[16:24])

def edit(path, change):
    p = parts(path); change(p); write(path, p.items())

def add_default(p, ext, kind):
    p['[Content_Types].xml'] = p['[Content_Types].xml'].replace(b'</Types>', f'<Default Extension="{ext}" ContentType="{kind}"/></Types>'.encode())

def add_override(p, name, kind):
    p['[Content_Types].xml'] = p['[Content_Types].xml'].replace(b'</Types>', f'<Override PartName="{name}" ContentType="{kind}"/></Types>'.encode())

def add_presentation_rel(p, rid, kind, target):
    p['ppt/_rels/presentation.xml.rels'] = p['ppt/_rels/presentation.xml.rels'].replace(b'</Relationships>', f'<Relationship Id="{rid}" Type="{kind}" Target="{target}"/></Relationships>'.encode())

def report(output): return json.loads(Path(str(output) + '.report.json').read_text())

class Work(unittest.TestCase):
    def setUp(self):
        self.d = Path(tempfile.mkdtemp())
    def tearDown(self):
        shutil.rmtree(self.d, ignore_errors=True)

# 1. ZIP: the names that are checked must be the names that are extracted.
class ZipNames(Work):
    def merge(self, change_b=None, patch=None):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        fixture(a); fixture(b)
        if change_b: edit(b, change_b)
        if patch: b.write_bytes(patch(b.read_bytes()))
        return run('--merge', o, a, b), o

    def test_a_local_header_renamed_from_the_central_directory_is_refused_or_ignored(self):
        # The first copy of the name is the local header; the central directory keeps the real name.
        r, o = self.merge(patch=lambda data: data.replace(b'ppt/slides/slide1.xml', b'ppt/slides/slide9.xml', 1))
        if WINDOWS_ENGINE and r.returncode == 0:
            # The Windows reader extracts by the central directory: nothing hidden comes through.
            self.assertNotIn('ppt/slides/slide9.xml', zipfile.ZipFile(o).namelist())
        else:
            self.assertEqual(r.returncode, 1, r.stderr); self.assertFalse(o.exists())

    def test_names_with_control_characters_are_refused(self):
        def change(p): p['ppt/media/a\nb.png'] = b'x'
        r, o = self.merge(change)
        self.assertEqual(r.returncode, 1, r.stderr); self.assertIn('unsupported ZIP entry path', r.stderr)

    def test_names_with_accents_still_work(self):
        def change(p):
            add_default(p, 'png', 'image/png'); p['ppt/media/café.png'] = png(4, 4)
        r, o = self.merge(change)
        self.assertEqual(r.returncode, 0, r.stderr)
        # ditto writes UTF-8 names without the UTF-8 flag, so Python reads them as CP437.
        names = [i.filename if i.flag_bits & 0x800 else i.filename.encode('cp437').decode('utf-8') for i in zipfile.ZipFile(o).infolist()]
        self.assertIn('pptMerger/source1/ppt/media/café.png', names)

# 2. Merge: an imported package's relationships cannot reach the first presentation's parts.
class ImportedTargets(Work):
    def decks(self):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        fixture(a); fixture(b)
        def base(p): add_default(p, 'png', 'image/png'); p['ppt/media/img.png'] = png(2, 2, (0, 0, 255))
        def imported(p):
            add_default(p, 'png', 'image/png'); p['ppt/media/img.png'] = png(2, 2, (255, 0, 0))
            p['ppt/theme/theme1.xml'] = f'<a:theme xmlns:a="{A}" name="T"><a:themeElements/></a:theme>'.encode()
            add_override(p, '/ppt/theme/theme1.xml', 'application/vnd.openxmlformats-officedocument.theme+xml')
            add_presentation_rel(p, 'rTheme', f'{R}/theme', 'theme/theme1.xml')
            # Relative, climbing above the package root, and absolute with "..".
            p['ppt/theme/_rels/theme1.xml.rels'] = rels(('rImg', IMAGE, '../../../../ppt/media/img.png')).encode()
            p['ppt/slides/_rels/slide1.xml.rels'] = rels(('rImg', IMAGE, '/../../ppt/media/img.png')).encode()
        edit(a, base); edit(b, imported)
        return a, b, o

    def test_targets_stay_inside_the_imported_package(self):
        a, b, o = self.decks()
        r = run('--merge', o, a, b)
        self.assertEqual(r.returncode, 0, r.stderr)
        with zipfile.ZipFile(o) as z:
            for part in ['pptMerger/source1/ppt/theme/_rels/theme1.xml.rels', 'pptMerger/source1/ppt/slides/_rels/slide1.xml.rels']:
                self.assertIn(b'Target="/pptMerger/source1/ppt/media/img.png"', z.read(part), part)
        self.assertTrue(all(not v['issues'] for v in report(o)['validation']), report(o)['validation'])

    def test_the_verifier_notices_a_target_that_escapes(self):
        a, b, o = self.decks()
        self.assertEqual(run('--merge', o, a, b).returncode, 0)
        bad = self.d / 'bad.pptx'
        p = parts(o)
        p['pptMerger/source1/ppt/theme/_rels/theme1.xml.rels'] = rels(('rImg', IMAGE, '../../../../ppt/media/img.png')).encode()
        write(bad, p.items())
        r = run('--verify', bad, a, b)
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr); self.assertIn('Wrongly linked resource', r.stdout)

# 3. The logs folder: only the app's own old files are deleted.
class LogCleanup(Work):
    def test_other_files_in_the_folder_are_kept(self):
        logs = self.d / 'logs'; logs.mkdir()
        old = time.time() - 40 * 86400
        for name in ['notes.txt', 'PPTShowTools-other.log', 'old.report.json', 'old.report.txt', 'PPTShowTools-ERROR-1234.log']:
            (logs / name).write_text('x'); os.utime(logs / name, (old, old))
        a, b = self.d / 'a.pptx', self.d / 'b.pptx'; fixture(a); fixture(b)
        self.assertEqual(run_installed('--merge', self.d / 'o.pptx', a, b, PPTSHOWTOOLS_LOG_DIR=str(logs)).returncode, 0)
        names = {p.name for p in logs.iterdir()}
        self.assertTrue({'notes.txt', 'PPTShowTools-other.log'} <= names, names)
        self.assertFalse({'old.report.json', 'old.report.txt', 'PPTShowTools-ERROR-1234.log'} & names, names)

# 4. Pictures: very large ones are reduced or refused before any full-size RGBA buffer.
class PictureLimits(Work):
    def resize_with_background(self, picture):
        deck, out = self.d / 'deck.pptx', self.d / 'wide.pptx'
        fixture(deck, 9144000, 6858000)
        def change(p):
            add_default(p, 'png', 'image/png'); p['ppt/media/bg.png'] = picture
            p['ppt/slides/slide1.xml'] = f'<p:sld xmlns:p="{P}" xmlns:a="{A}" xmlns:r="{R}"><p:cSld><p:bg><p:bgPr><a:blipFill><a:blip r:embed="rBg"/><a:stretch><a:fillRect/></a:stretch></a:blipFill><a:effectLst/></p:bgPr></p:bg><p:spTree/></p:cSld></p:sld>'.encode()
            p['ppt/slides/_rels/slide1.xml.rels'] = rels(('rBg', IMAGE, '../media/bg.png')).encode()
        edit(deck, change)
        r = run('--resize', '--size', '1920x1080', out, deck)
        self.assertEqual(r.returncode, 0, r.stderr)
        return parts(out)

    def test_a_large_picture_is_reduced_before_padding(self):
        made = self.resize_with_background(png(7000, 6000))
        padded = [n for n in made if re.fullmatch(r'ppt/media/pst-pad-[0-9a-f]{8}\.png', n)]
        self.assertEqual(len(padded), 1, list(made))
        w, h = png_size(made[padded[0]])
        self.assertLessEqual(w * h, 36_000_000 * 1.01)

    def test_a_huge_picture_is_not_decoded(self):
        made = self.resize_with_background(png(13000, 10000))
        self.assertFalse([n for n in made if 'pst-pad-' in n])
        self.assertEqual(made['ppt/media/bg.png'][:8], b'\x89PNG\r\n\x1a\n')

# 5. Macros are found by content and relationship type, not only by the usual name.
class MacroParts(Work):
    def with_macros(self, path):
        fixture(path)
        def change(p):
            p['ppt/macros/project.bin'] = b'VBA'
            add_override(p, '/ppt/macros/project.bin', 'application/vnd.ms-office.vbaProject')
            add_presentation_rel(p, 'rVba', 'http://schemas.microsoft.com/office/2006/relationships/vbaProject', 'macros/project.bin')
            p['customXml/item1.xml'] = b'<data>private</data>'
            add_override(p, '/customXml/item1.xml', 'application/xml')
        edit(path, change)

    def test_an_imported_vba_project_with_another_name_is_removed(self):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        fixture(a); self.with_macros(b)
        r = run('--merge', o, a, b)
        self.assertEqual(r.returncode, 0, r.stderr)
        names = zipfile.ZipFile(o).namelist()
        self.assertFalse([n for n in names if 'macros/project.bin' in n or n.startswith('pptMerger/source1/customXml')], names)
        self.assertTrue(any('VBA project (macros) was removed' in w for w in report(o)['warnings']))

    def test_a_first_file_with_renamed_macros_needs_pptm(self):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        self.with_macros(a); fixture(b)
        r = run('--merge', o, a, b)
        self.assertEqual(r.returncode, 1); self.assertIn('contains VBA', r.stderr)

    # 11. "Merge and save with PowerPoint…" says why it cannot keep macros, before PowerPoint opens.
    def test_merge_with_powerpoint_explains_macros(self):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        self.with_macros(a); fixture(b)
        r = run('--merge-powerpoint', o, a, b)
        self.assertEqual(r.returncode, 1); self.assertIn('contains macros (VBA)', r.stderr)

# 6. Targets are resolved on the text alone, never against the Mac's own folders.
class LexicalTargets(Work):
    def test_a_target_through_a_system_folder_name_resolves_inside_the_package(self):
        deck, out = self.d / 'deck.pptx', self.d / 'media'
        out.mkdir(); fixture(deck)
        def change(p):
            add_default(p, 'png', 'image/png'); p['ppt/media/image1.png'] = png(2, 2)
            p['ppt/slides/_rels/slide1.xml.rels'] = rels(('rImg', IMAGE, '/var/../ppt/media/image1.png')).encode()
        edit(deck, change)
        r = run('--extract-media', '--kinds', 'images', out, deck)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(list(next(out.iterdir()).iterdir())), 1)

# 7. A blurred copy that cannot be made leaves no file behind and the resize still works.
class FailedBackdrop(Work):
    def test_no_partial_video_is_left_and_the_still_is_used(self):
        folder = self.d / 'src'; folder.mkdir()
        (folder / 'clip.mp4').write_bytes(b'not a video'); (folder / 'poster.png').write_bytes(png(64, 36))
        deck = self.d / 'video.pptx'
        subprocess.run(['python3', str(Path(__file__).with_name('video_deck.py')), str(folder), str(deck)], check=True)
        # A QuickTime clip: the package declares mov, not mp4.
        def change(p):
            p['[Content_Types].xml'] = p['[Content_Types].xml'].replace(b'Extension="mp4" ContentType="video/mp4"', b'Extension="mov" ContentType="video/quicktime"')
            p['ppt/media/media1.mov'] = p.pop('ppt/media/media1.mp4')
            p['ppt/slides/_rels/slide1.xml.rels'] = p['ppt/slides/_rels/slide1.xml.rels'].replace(b'media1.mp4', b'media1.mov')
        edit(deck, change)
        env = {}
        if WINDOWS_ENGINE:
            # An encoder that writes part of a file and then fails.
            fake = self.d / 'ffmpeg'
            fake.write_text('#!/bin/sh\nfor last; do :; done\necho partial > "$last"\nexit 1\n'); fake.chmod(0o755)
            env['PPTSHOWTOOLS_FFMPEG'] = str(fake)
        out = self.d / 'wide.pptx'
        r = subprocess.run([str(BIN), '--resize', '--size', '3427x534', str(out), str(deck)], capture_output=True, text=True, env={**os.environ, **env})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse([n for n in parts(out) if n.endswith('.mp4')])

# 8. Resizing again replaces the blurred background; Extract media leaves it out.
@unittest.skipUnless(FFMPEG, 'FFmpeg is needed to make the test video')
class ResizeTwice(Work):
    def test_one_background_after_two_resizes_and_none_extracted(self):
        subprocess.run([FFMPEG, '-nostdin', '-loglevel', 'error', '-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=25', '-t', '1', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', str(self.d / 'clip.mp4')], check=True)
        subprocess.run([FFMPEG, '-nostdin', '-loglevel', 'error', '-i', str(self.d / 'clip.mp4'), '-frames:v', '1', str(self.d / 'poster.png')], check=True)
        deck, wide, square = self.d / 'video.pptx', self.d / 'wide.pptx', self.d / 'square.pptx'
        subprocess.run(['python3', str(Path(__file__).with_name('video_deck.py')), str(self.d), str(deck)], check=True)
        self.assertEqual(run('--resize', '--size', '3427x534', wide, deck).returncode, 0)
        r = run('--resize', '--size', '1024x768', square, wide)
        self.assertEqual(r.returncode, 0, r.stderr)
        slide = parts(square)['ppt/slides/slide1.xml'].decode()
        self.assertEqual(slide.count(f'name="{BACKDROP}"'), 1)
        out = self.d / 'media'; out.mkdir()
        self.assertEqual(run('--extract-media', out, wide).returncode, 0)
        files = sorted(p.name for p in next(out.iterdir()).iterdir())
        self.assertEqual(len(files), 1, files)
        self.assertTrue(files[0].endswith('.mp4'))

# 9. Part names in [Content_Types].xml are compared without letter case.
class ContentTypeCase(Work):
    def test_an_override_in_another_case_is_found(self):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        fixture(a); fixture(b)
        edit(b, lambda p: p.__setitem__('[Content_Types].xml', p['[Content_Types].xml'].replace(b'/ppt/slides/slide1.xml', b'/ppt/slides/Slide1.xml')))
        r = run('--merge', o, a, b)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(b'PartName="/pptMerger/source1/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"', parts(o)['[Content_Types].xml'])

# 10. A carriage return in the notes survives Extract notes and its check.
class NotesCarriageReturn(Work):
    def test_notes_with_a_carriage_return(self):
        deck, out = self.d / 'notes.pptx', self.d / 'out.pptx'
        notes_fixture(deck)
        edit(deck, lambda p: p.__setitem__('ppt/notesSlides/notesSlide1.xml', re.sub(rb'<a:t>Primera[^<]*</a:t>', b'<a:t>one&#13;two</a:t>', p['ppt/notesSlides/notesSlide1.xml'])))
        r = run('--extract-notes', out, deck)
        self.assertEqual(r.returncode, 0, r.stderr)

# 12. Animation formulas with "#ppt_x" and motion paths with signs between numbers are scaled.
class AnimationFormulas(Work):
    def test_hash_variables_and_packed_numbers_are_scaled(self):
        deck, out = self.d / 'deck.pptx', self.d / 'wide.pptx'
        fixture(deck, 9144000, 6858000)
        timing = ('<p:timing><p:tnLst><p:par><p:cTn id="1"><p:childTnLst>'
                  '<p:animMotion origin="layout" path="M 0 0 L 0.5-0.25 E"><p:cBhvr><p:cTn id="2" dur="1000"/><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cBhvr></p:animMotion>'
                  '<p:anim calcmode="lin" valueType="num"><p:cBhvr><p:cTn id="3" dur="1000"/><p:tgtEl><p:spTgt spid="2"/></p:tgtEl><p:attrNameLst><p:attrName>ppt_x</p:attrName></p:attrNameLst></p:cBhvr>'
                  '<p:tavLst><p:tav tm="0" fmla="#ppt_x+0.1"><p:val><p:fltVal val="0"/></p:val></p:tav></p:tavLst></p:anim>'
                  '</p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>')
        slide = f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Box"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld>{timing}</p:sld>'
        edit(deck, lambda p: p.__setitem__('ppt/slides/slide1.xml', slide.encode()))
        r = run('--resize', '--size', '1280x720', out, deck)
        self.assertEqual(r.returncode, 0, r.stderr)
        xml = parts(out)['ppt/slides/slide1.xml'].decode()
        # 4:3 into 16:9 at the same height: horizontal fractions × 0.75.
        self.assertIn('fmla="#ppt_x+0.075"', xml)
        self.assertIn('path="M 0 0 L 0.375 -0.25 E"', xml)

# 13. XML and command-line details.
class Details(Work):
    def test_a_doctype_inside_a_comment_is_only_text(self):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        fixture(a); fixture(b)
        edit(b, lambda p: p.__setitem__('ppt/slides/slide1.xml', b'<!-- <!DOCTYPE x> --><p:sld xmlns:p="' + P.encode() + b'"><p:cSld><p:spTree/></p:cSld></p:sld>'))
        r = run('--merge', o, a, b)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_verify_checks_the_output_size(self):
        a, b, o = self.d / 'a.pptx', self.d / 'b.pptx', self.d / 'o.pptx'
        fixture(a); fixture(b)
        self.assertEqual(run('--merge', '--size', '1920x1080', o, a, b).returncode, 0)
        self.assertEqual(run('--verify', '--size', '1920x1080', o, a, b).returncode, 0)
        r = run('--verify', '--size', '4800x1344', o, a, b)
        self.assertEqual(r.returncode, 2); self.assertIn("slide size", r.stdout)

    def test_office_prepare_uses_the_chosen_size(self):
        a, b = self.d / 'a.pptx', self.d / 'b.pptx'
        fixture(a); fixture(b)
        r = run('--office-prepare', '--size', '1920x1080', self.d / 'prepared', a, b)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((json.loads(r.stdout)['width'], json.loads(r.stdout)['height']), (1920 * 9525, 1080 * 9525))

if __name__ == '__main__':
    unittest.main()

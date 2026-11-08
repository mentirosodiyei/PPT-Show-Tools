"""Security review, November 2026: hostile packages must be refused cleanly
(never crash, never write outside the output), and a merge must not carry
along macros or metadata from the other presentations."""
import json, stat, subprocess, tempfile, unittest, zipfile
from pathlib import Path
from test_guards import fixture, BIN
from test_slide_size import deck

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
WINDOWS_ENGINE = 'windows-target' in str(BIN)

def parts(path):
    with zipfile.ZipFile(path) as z:
        return {n: z.read(n) for n in z.namelist()}

def write(path, items):
    with zipfile.ZipFile(path, 'w') as z:
        for name, data in items:
            if isinstance(name, zipfile.ZipInfo):
                z.writestr(name, data)
            else:
                z.writestr(name, data)

def run(*args):
    return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True, timeout=120)

class HostileXml(unittest.TestCase):
    def merge_with_slide(self, data):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b, 9144000, 6858000)
            p = parts(b); p['ppt/slides/slide1.xml'] = data; write(b, p.items())
            r = run('--merge', o, a, b)
            self.assertFalse(o.exists())
            return r

    def test_deep_nesting_is_refused_without_crashing(self):
        r = self.merge_with_slide((f'<p:sld xmlns:p="{P}"><p:cSld><p:spTree>' + '<x>' * 100000 + '</x>' * 100000 + '</p:spTree></p:cSld></p:sld>').encode())
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn('nested too deeply', r.stderr)

    def test_other_encodings_are_refused(self):
        for data in [
            ('<?xml version="1.0" encoding="UTF-16"?>' + f'<p:sld xmlns:p="{P}"/>').encode('utf-16'),
            ('<?xml version="1.0" encoding="UTF-7"?>+ADw-!DOCTYPE x+AD4-' + f'<p:sld xmlns:p="{P}"/>').encode(),
            ('<?xml version="1.0" encoding="ISO-8859-1"?>' + f'<p:sld xmlns:p="{P}"/>').encode(),
        ]:
            r = self.merge_with_slide(data)
            self.assertEqual(r.returncode, 1, r.stderr)
            self.assertIn('encoding', r.stderr)

    def test_entity_expansion_is_refused(self):
        bomb = '<!doctype p:sld [<!ENTITY a "aaaaaaaaaa">' + ''.join(f'<!ENTITY {chr(98 + i)} "' + ('&' + chr(97 + i) + ';') * 10 + '">' for i in range(8)) + ']>'
        r = self.merge_with_slide((bomb + f'<p:sld xmlns:p="{P}"><p:cSld><p:spTree>&i;</p:spTree></p:cSld></p:sld>').encode())
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn('document type declaration', r.stderr)

class HostileZip(unittest.TestCase):
    def merge_with_entries(self, extra):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); work = d / 'work'; work.mkdir()
            a, b, o = work / 'a.pptx', work / 'b.pptx', work / 'o.pptx'
            fixture(a); fixture(b)
            write(b, list(parts(b).items()) + extra)
            r = run('--merge', o, a, b)
            self.assertFalse(o.exists())
            self.assertEqual(sorted(p.name for p in d.iterdir()), ['work'])
            return r

    def test_a_file_used_as_a_folder_is_refused(self):
        link = zipfile.ZipInfo('ppt/media/link'); link.create_system = 3; link.external_attr = (stat.S_IFREG | 0o644) << 16
        r = self.merge_with_entries([(link, b'/tmp'), ('ppt/media/link/written.txt', b'x')])
        self.assertEqual(r.returncode, 1); self.assertIn('unsupported ZIP entry path', r.stderr)

    def test_parent_folders_are_refused_even_as_folder_entries(self):
        r = self.merge_with_entries([(zipfile.ZipInfo('ppt/../../escaped/'), b'')])
        self.assertEqual(r.returncode, 1); self.assertIn('unsupported ZIP entry path', r.stderr)

    @unittest.skipUnless(WINDOWS_ENGINE, 'reserved device names only matter on Windows')
    def test_windows_device_names_are_refused(self):
        for entry in ['ppt/media/CON', 'ppt/media/nul.png', 'ppt/media/COM1.xml', 'ppt/media/image.png.', 'ppt/media/a<b.png']:
            r = self.merge_with_entries([(entry, b'x')])
            self.assertEqual(r.returncode, 1, entry); self.assertIn('unsupported ZIP entry path', r.stderr)

class MergedContent(unittest.TestCase):
    def test_metadata_and_macros_of_other_files_are_not_carried_along(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            p = parts(b)
            p['docProps/core.xml'] = b'<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Secret Author</dc:creator></cp:coreProperties>'
            p['docProps/thumbnail.jpeg'] = b'\xff\xd8\xff'
            p['ppt/vbaProject.bin'] = b'macro code'
            p['[Content_Types].xml'] = p['[Content_Types].xml'].replace(b'</Types>', b'<Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>')
            write(b, p.items())
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            names = parts(o)
            self.assertFalse([n for n in names if n.startswith('pptMerger/source1/docProps') or n.endswith('vbaProject.bin')], sorted(names))
            self.assertNotIn(b'Secret Author', b''.join(names.values()))
            report = json.loads(Path(str(o) + '.report.json').read_text())
            self.assertTrue(any('VBA project (macros) was removed' in w for w in report['warnings']), report['warnings'])
            self.assertTrue(all(not v['issues'] for v in report['validation']), report['validation'])

    def test_linked_files_outside_the_presentation_are_reported(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            p = parts(b)
            rel = b'<Relationship Id="rLinked" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="file://server/share/picture.png" TargetMode="External"/><Relationship Id="rWeb" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/>'
            rels = p.get('ppt/slides/_rels/slide1.xml.rels', b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>')
            p['ppt/slides/_rels/slide1.xml.rels'] = rels.replace(b'</Relationships>', rel + b'</Relationships>')
            write(b, p.items())
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            warnings = json.loads(Path(str(o) + '.report.json').read_text())['warnings']
            linked = [w for w in warnings if 'outside the presentation' in w]
            self.assertEqual(len(linked), 1, warnings)
            self.assertIn('file://server/share/picture.png', linked[0])
            self.assertNotIn('example.com', linked[0])

class PdfLimits(unittest.TestCase):
    def test_pdfs_with_too_many_pages_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, o = d / 'many.pdf', d / 'o.pptx'
            pages = 2001
            objects = [b'<< /Type /Catalog /Pages 2 0 R >>', ('<< /Type /Pages /Kids [' + ' '.join(f'{3 + i} 0 R' for i in range(pages)) + f'] /Count {pages} >>').encode()]
            objects += [b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>'] * pages
            pdf = b'%PDF-1.4\n'; offsets = []
            for i, obj in enumerate(objects, 1):
                offsets.append(len(pdf)); pdf += f'{i} 0 obj\n'.encode() + obj + b'\nendobj\n'
            xref = len(pdf); pdf += f'xref\n0 {len(objects) + 1}\n0000000000 65535 f \n'.encode() + b''.join(f'{x:010} 00000 n \n'.encode() for x in offsets)
            pdf += f'trailer << /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF'.encode(); a.write_bytes(pdf)
            r = run('--pdf-to-ppt', o, a)
            if 'Windows renderer' in r.stderr:
                self.skipTest('PDF rendering needs the Windows renderer in this build')
            self.assertEqual(r.returncode, 1, r.stderr); self.assertIn('at most 2000', r.stderr); self.assertFalse(o.exists())

if __name__ == '__main__':
    unittest.main()

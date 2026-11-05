"""Regression tests for the findings of the November 2026 code audit."""
import json, os, struct, subprocess, tempfile, unittest, zipfile
from pathlib import Path
import xml.etree.ElementTree as E
from test_guards import fixture, BIN
from test_slide_size import deck

P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'

def parts(path):
    with zipfile.ZipFile(path) as z:
        return {n: z.read(n) for n in z.namelist()}

def write(path, items):
    with zipfile.ZipFile(path, 'w') as z:
        for name, data in items:
            z.writestr(name, data)

def run(*args, env=None):
    return subprocess.run([str(BIN), *map(str, args)], capture_output=True, text=True, env={**os.environ, **(env or {})})

def slide(body):
    return f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree>{body}</p:spTree></p:cSld></p:sld>'.encode()

def text_shape(t):
    return f'<p:sp><p:nvSpPr><p:cNvPr id="2" name="T"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:txBody></p:sp>'

class Packages(unittest.TestCase):
    def test_entries_that_differ_only_in_case_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            p = parts(b)
            write(b, list(p.items()) + [('PPT/SLIDES/SLIDE1.XML', p['ppt/slides/slide1.xml'])])
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 1); self.assertIn('duplicate ZIP entry', r.stderr); self.assertFalse(o.exists())

    def test_parent_paths_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            write(b, list(parts(b).items()) + [('ppt/../../escaped.txt', b'x')])
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 1); self.assertIn('unsupported ZIP entry path', r.stderr)

    def test_real_size_limit_even_when_the_zip_understates_it(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            p = parts(b)
            with zipfile.ZipFile(b, 'w', zipfile.ZIP_DEFLATED) as z:
                for n, v in p.items():
                    z.writestr(n, v)
                z.writestr('ppt/media/big.bin', b'\0' * 3_000_000)
            env = {'DECKWRIGHT_MAX_PACKAGE_BYTES': '500000'}
            honest = run('--merge', o, a, b, env=env)
            self.assertEqual(honest.returncode, 1); self.assertIn('too large', honest.stderr)
            # Rewrite the declared size of the big entry to 10 bytes in both headers.
            data = bytearray(b.read_bytes())
            for sig, offset in [(b'PK\x01\x02', 24), (b'PK\x03\x04', 22)]:
                i = 0
                while (i := data.find(sig, i)) != -1:
                    name_len = struct.unpack_from('<H', data, i + (28 if sig == b'PK\x01\x02' else 26))[0]
                    name_at = i + (46 if sig == b'PK\x01\x02' else 30)
                    if data[name_at:name_at + name_len] == b'ppt/media/big.bin':
                        struct.pack_into('<I', data, i + offset, 10)
                    i += 4
            b.write_bytes(bytes(data))
            lying = run('--merge', o, a, b, env=env)
            self.assertEqual(lying.returncode, 1, lying.stderr); self.assertFalse(o.exists())

class XmlParts(unittest.TestCase):
    def test_document_type_declarations_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            p = parts(b)
            p['ppt/slides/slide1.xml'] = b'<?xml version="1.0"?><!DOCTYPE x [<!ENTITY e "EXPANDED">]>' + slide(text_shape('&e;'))
            write(b, p.items())
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 1); self.assertIn('document type declaration', r.stderr); self.assertFalse(o.exists())

    def test_comments_cdata_and_line_breaks_survive_rewriting(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b, 9144000, 6858000)  # 4:3, so its slide is rewritten
            p = parts(b)
            # A quote inside a comment, CDATA with a line break, a CRLF in text and a
            # line break written as a character reference in an attribute.
            shape = text_shape('<![CDATA[line1\nline2]]>').replace('name="T"', 'name="T" descr="x&#10;y"')
            shape = shape.replace('</a:p></p:txBody>', '</a:p><a:p><a:r><a:t>crlf\r\nend</a:t></a:r></a:p></p:txBody>')
            p['ppt/slides/slide1.xml'] = slide('<!-- it\'s "odd -->' + shape)
            write(b, p.items())
            r = run('--merge', '--fit-non169', o, a, b)
            self.assertEqual(r.returncode, 0, r.stderr)
            raw = parts(o)['pptMerger/source1/ppt/slides/slide1.xml']
            self.assertIn(b'<!-- it\'s "odd -->', raw)
            root = E.fromstring(raw)
            texts = [t.text for t in root.iter('{' + A + '}t')]
            self.assertEqual(texts, ['line1\nline2', 'crlf\nend'])
            self.assertEqual(next(e for e in root.iter() if e.tag.endswith('}cNvPr')).get('descr'), 'x\ny')

class SizesAndIds(unittest.TestCase):
    def test_slide_sizes_outside_limits_are_refused_without_crashing(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'huge.pptx', d / 'o.pptx'
            fixture(a); fixture(b, 9223372036854775807, 9223372036854775807)
            for args in [('--merge', '--fit-non169', o, a, b), ('--resize', o, b), ('--merge', '--size', '1920x1080', o, a, b)]:
                r = run(*args)
                self.assertEqual(r.returncode, 1, args); self.assertIn("outside PowerPoint's limits", r.stderr); self.assertFalse(o.exists())

    def test_highest_slide_id_is_refused_without_crashing(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'max.pptx', d / 'b.pptx', d / 'o.pptx'
            fixture(a); fixture(b)
            p = parts(a); p['ppt/presentation.xml'] = p['ppt/presentation.xml'].replace(b'id="256"', b'id="4294967295"'); write(a, p.items())
            r = run('--merge', o, a, b)
            self.assertEqual(r.returncode, 1); self.assertIn('Too many slide identifiers', r.stderr)

class Verifier(unittest.TestCase):
    def merged(self, d, with_font=False):
        a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
        fixture(a); fixture(b)
        if with_font:
            p = parts(b)
            p['ppt/presentation.xml'] = p['ppt/presentation.xml'].replace(b'</p:presentation>', b'<p:embeddedFontLst><p:embeddedFont><p:font typeface="Test Embedded"/><p:regular r:id="fontRel"/></p:embeddedFont></p:embeddedFontLst></p:presentation>')
            p['ppt/_rels/presentation.xml.rels'] = p['ppt/_rels/presentation.xml.rels'].replace(b'</Relationships>', b'<Relationship Id="fontRel" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/font1.fntdata"/></Relationships>')
            p['[Content_Types].xml'] = p['[Content_Types].xml'].replace(b'</Types>', b'<Default Extension="fntdata" ContentType="application/x-fontdata"/></Types>')
            p['ppt/fonts/font1.fntdata'] = b'font'
            write(b, p.items())
        r = run('--merge', o, a, b)
        self.assertEqual(r.returncode, 0, r.stderr)
        return a, b, o

    def test_missing_content_types_is_an_issue(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = self.merged(d)
            p = parts(o); p.pop('[Content_Types].xml'); bad = d / 'bad.pptx'; write(bad, p.items())
            r = run('--verify', bad, a, b)
            self.assertEqual(r.returncode, 2); self.assertIn('[Content_Types].xml', json.dumps(json.loads(r.stdout), ensure_ascii=False))

    def test_dropped_embedded_font_is_an_issue(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = self.merged(d, with_font=True)
            self.assertEqual(run('--verify', o, a, b).returncode, 0)
            p = parts(o)
            root = p['ppt/presentation.xml'].decode()
            start, end = root.index('<p:embeddedFontLst>'), root.index('</p:embeddedFontLst>') + len('</p:embeddedFontLst>')
            p['ppt/presentation.xml'] = (root[:start] + root[end:]).encode()
            bad = d / 'bad.pptx'; write(bad, p.items())
            r = run('--verify', bad, a, b)
            self.assertEqual(r.returncode, 2); self.assertIn('Test Embedded', r.stdout)

    def test_verify_uses_the_size_the_output_was_merged_with(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); a, b, o = d / 'a.pptx', d / 'b.pptx', d / 'o.pptx'
            deck(a, 12192000, 6858000, motion=True); deck(b, 12192000, 6858000)
            self.assertEqual(run('--merge', '--size', '4800x1344', o, a, b).returncode, 0)
            r = run('--verify', '--size', '4800x1344', o, a, b)
            self.assertEqual(r.returncode, 0, r.stdout)

class Outputs(unittest.TestCase):
    def test_error_log_goes_next_to_the_output(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d); src, out = d / 'in', d / 'out'
            src.mkdir(); out.mkdir()
            bad = src / 'bad.pptx'; bad.write_bytes(b'not a zip')
            r = run('--extract-notes', out / 'notes.pptx', bad)
            self.assertEqual(r.returncode, 1)
            self.assertFalse(list(src.glob('Deckwright-ERROR-*.log')))
            self.assertTrue(list(out.glob('Deckwright-ERROR-*.log')))

if __name__ == '__main__':
    unittest.main()

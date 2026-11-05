#!/usr/bin/env python3
"""Tests for SlideScaler (4:3 fit, AlternateContent, animMotion, insets) and Verifier (spid integrity)."""
import json, subprocess, tempfile, unittest, zipfile
import xml.etree.ElementTree as E
from pathlib import Path
from test_guards import fixture, BIN


class ScalerAndEngineTests(unittest.TestCase):
    def test_adapt_4_3_into_16_9_keeps_animations_and_alternate_content(self):
        with tempfile.TemporaryDirectory() as d:
            a, b, o = [Path(d) / x for x in ('wide.pptx', 'fourthree.pptx', 'out.pptx')]
            fixture(a, 12192000, 6858000)
            fixture(b, 9144000, 6858000)
            slide_xml = '''<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
                                  xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
                                  xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
              <p:cSld>
                <p:spTree>
                  <p:sp>
                    <p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
                    <p:spPr><a:xfrm><a:off x="0" y="100"/><a:ext cx="9144000" cy="6858000"/></a:xfrm></p:spPr>
                    <p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr sz="2400"/><a:t>4:3 Text</a:t></a:r></a:p></p:txBody>
                  </p:sp>
                  <p:grpSp>
                    <p:nvGrpSpPr><p:cNvPr id="3" name="Group"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
                    <p:grpSpPr><a:xfrm><a:off x="1000" y="2000"/><a:ext cx="500" cy="500"/><a:chOff x="1000" y="2000"/><a:chExt cx="500" cy="500"/></a:xfrm></p:grpSpPr>
                    <p:sp>
                      <p:nvSpPr><p:cNvPr id="4" name="Child"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
                      <p:spPr><a:xfrm><a:off x="1100" y="2100"/><a:ext cx="100" cy="100"/></a:xfrm></p:spPr>
                    </p:sp>
                  </p:grpSp>
                  <mc:AlternateContent>
                    <mc:Choice Requires="p14">
                      <p:sp>
                        <p:nvSpPr><p:cNvPr id="5" name="ModernChoice"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
                        <p:spPr><a:xfrm><a:off x="500" y="300"/><a:ext cx="200" cy="200"/></a:xfrm></p:spPr>
                      </p:sp>
                    </mc:Choice>
                    <mc:Fallback>
                      <p:sp>
                        <p:nvSpPr><p:cNvPr id="5" name="ModernFallback"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
                        <p:spPr><a:xfrm><a:off x="500" y="300"/><a:ext cx="200" cy="200"/></a:xfrm></p:spPr>
                      </p:sp>
                    </mc:Fallback>
                  </mc:AlternateContent>
                  <p:graphicFrame>
                    <p:nvGraphicFramePr><p:cNvPr id="6" name="Table"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
                    <p:xfrm><a:off x="0" y="0"/><a:ext cx="1000" cy="1000"/></p:xfrm>
                    <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">
                      <a:tbl><a:tr h="370840"><a:tc><a:txBody><a:bodyPr/><a:p/></a:txBody><a:tcPr/></a:tc></a:tr></a:tbl>
                    </a:graphicData></a:graphic>
                  </p:graphicFrame>
                </p:spTree>
              </p:cSld>
              <p:timing>
                <p:tnLst>
                  <p:par>
                    <p:cTn id="1" dur="indefinite">
                      <p:childTnLst>
                        <p:animMotion origin="layout" path="M 0 0 L 0.25 0.1 E">
                          <p:cBhvr><p:cTn id="2" dur="2000"/><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cBhvr>
                        </p:animMotion>
                        <p:animMotion origin="parent" path="M 0 0 L 0.25 0.1 E">
                          <p:cBhvr><p:cTn id="3" dur="2000"/><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cBhvr>
                        </p:animMotion>
                        <p:anim calcmode="lin" valueType="num">
                          <p:cBhvr><p:cTn id="4" dur="1000"/><p:tgtEl><p:spTgt spid="2"/></p:tgtEl></p:cBhvr>
                          <p:tavLst><p:tav tm="100000" fmla="ppt_x+0.2"/></p:tavLst>
                        </p:anim>
                      </p:childTnLst>
                    </p:cTn>
                  </p:par>
                </p:tnLst>
              </p:timing>
            </p:sld>'''
            with zipfile.ZipFile(b) as z:
                parts = {n: z.read(n) for n in z.namelist()}
            parts['ppt/slides/slide1.xml'] = slide_xml.encode()
            with zipfile.ZipFile(b, 'w') as z:
                for n, data in parts.items():
                    z.writestr(n, data)

            r = subprocess.run([str(BIN), '--merge', str(o), str(a), str(b)], capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

            ns = {
                'p': 'http://schemas.openxmlformats.org/presentationml/2006/main',
                'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
                'mc': 'http://schemas.openxmlformats.org/markup-compatibility/2006',
            }
            with zipfile.ZipFile(o) as z:
                root = E.fromstring(z.read('pptMerger/source1/ppt/slides/slide1.xml'))
                for bodyPr in root.findall('.//a:bodyPr', ns):
                    self.assertEqual(bodyPr.attrib, {})
                for tcPr in root.findall('.//a:tcPr', ns):
                    self.assertEqual(tcPr.attrib, {})
                self.assertEqual(root.find('.//a:rPr', ns).get('sz'), '2400')
                # The text box takes the new proportion; the text keeps its size.
                title_sp = root.find('.//p:sp', ns)
                self.assertEqual(title_sp.find('.//a:off', ns).attrib, {'x': '0', 'y': '100'})
                self.assertEqual(title_sp.find('.//a:ext', ns).attrib, {'cx': '12192000', 'cy': '6858000'})
                # A small group near the top-left corner stays there, unchanged inside.
                grp = root.find('.//p:grpSp', ns)
                self.assertEqual(grp.find('p:grpSpPr/a:xfrm/a:off', ns).attrib, {'x': '1000', 'y': '2000'})
                self.assertEqual(grp.find('p:grpSpPr/a:xfrm/a:chOff', ns).attrib, {'x': '1000', 'y': '2000'})
                self.assertEqual(grp.find('p:sp/p:spPr/a:xfrm/a:off', ns).attrib, {'x': '1100', 'y': '2100'})
                choice_off = root.find('.//mc:Choice/p:sp/p:spPr/a:xfrm/a:off', ns)
                fallback_off = root.find('.//mc:Fallback/p:sp/p:spPr/a:xfrm/a:off', ns)
                self.assertEqual(choice_off.attrib, {'x': '500', 'y': '300'})
                self.assertEqual(fallback_off.attrib, {'x': '500', 'y': '300'})
                # Animation coordinates are fractions of the slide: they stay as they were.
                motions = root.findall('.//p:animMotion', ns)
                self.assertEqual(motions[0].get('path'), 'M 0 0 L 0.25 0.1 E')
                self.assertEqual(motions[1].get('path'), 'M 0 0 L 0.25 0.1 E')
                tav = root.find('.//p:tav', ns)
                self.assertEqual(tav.get('fmla'), 'ppt_x+0.2')

    def test_proportional_scaling_only_injects_bodypr_insets_on_master(self):
        with tempfile.TemporaryDirectory() as d:
            a, b, o = [Path(d) / x for x in ('small.pptx', 'large.pptx', 'out.pptx')]
            fixture(a, 9144000, 5143500)
            fixture(b, 18288000, 10287000)
            with zipfile.ZipFile(a) as z:
                parts = {n: z.read(n) for n in z.namelist()}
            parts['ppt/slides/slide1.xml'] = b'''<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="S"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="100" y="200"/><a:ext cx="300" cy="400"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="1800"/><a:t>Hi</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>'''
            parts['ppt/slideMasters/slideMaster1.xml'] = b'''<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:bodyPr/><a:p/></p:txBody></p:sp></p:spTree></p:cSld></p:sldMaster>'''
            parts['[Content_Types].xml'] = parts['[Content_Types].xml'].replace(
                b'</Types>',
                b'<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/></Types>'
            )
            with zipfile.ZipFile(a, 'w') as z:
                for n, data in parts.items():
                    z.writestr(n, data)

            r = subprocess.run([str(BIN), '--merge', str(o), str(a), str(b)], capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            ns = {'a': 'http://schemas.openxmlformats.org/drawingml/2006/main'}
            with zipfile.ZipFile(o) as z:
                sld = E.fromstring(z.read('ppt/slides/slide1.xml'))
                master = E.fromstring(z.read('ppt/slideMasters/slideMaster1.xml'))
                self.assertEqual(sld.find('.//a:bodyPr', ns).attrib, {})
                self.assertEqual(master.find('.//a:bodyPr', ns).get('lIns'), '182880')
                self.assertEqual(master.find('.//a:bodyPr', ns).get('tIns'), '91440')

    def test_verifier_detects_orphaned_spid(self):
        with tempfile.TemporaryDirectory() as d:
            a, b, o, bad = [Path(d) / x for x in ('a.pptx', 'b.pptx', 'out.pptx', 'bad.pptx')]
            fixture(a)
            fixture(b)
            with zipfile.ZipFile(b) as z:
                parts_b = {n: z.read(n) for n in z.namelist()}
            parts_b['ppt/slides/slide1.xml'] = b'''<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="42" name="AnimatedBox"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp></p:spTree></p:cSld><p:timing><p:tnLst><p:par><p:cTn id="1" dur="1000"><p:childTnLst><p:set><p:cBhvr><p:cTn id="2" dur="1"/><p:tgtEl><p:spTgt spid="42"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing></p:sld>'''
            with zipfile.ZipFile(b, 'w') as z:
                for n, data in parts_b.items():
                    z.writestr(n, data)

            r = subprocess.run([str(BIN), '--merge', str(o), str(a), str(b)], capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            with zipfile.ZipFile(o) as z:
                merged = {n: z.read(n) for n in z.namelist()}

            with zipfile.ZipFile(bad, 'w') as z:
                for n, content in merged.items():
                    if n == 'pptMerger/source1/ppt/slides/slide1.xml':
                        content = content.replace(b'id="42"', b'id="999"')
                    z.writestr(n, content)
            v = subprocess.run([str(BIN), '--verify', str(bad), str(a), str(b)], capture_output=True, text=True)
            self.assertEqual(v.returncode, 2)
            checks = json.loads(v.stdout)
            self.assertTrue(any('spid' in issue for issue in checks[1]['issues']))


if __name__ == '__main__':
    unittest.main()

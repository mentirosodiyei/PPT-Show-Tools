#!/usr/bin/env python3
import tempfile,zipfile,subprocess,unittest
from pathlib import Path
import os
# PPTMERGER_BIN runs the same suite against another build, such as the Windows engine.
BIN=Path(os.environ.get('PPTMERGER_BIN') or Path(__file__).resolve().parents[1]/'dist/PPT Merger.app/Contents/MacOS/PPTMerger')
def fixture(path,width=12192000,height=6858000):
 with zipfile.ZipFile(path,'w') as z:
  z.writestr('ppt/presentation.xml',f'<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="{width}" cy="{height}"/></p:presentation>')
  z.writestr('ppt/_rels/presentation.xml.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>')
  z.writestr('_rels/.rels','<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>')
  z.writestr('[Content_Types].xml','<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>')
  z.writestr('ppt/slides/slide1.xml','<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree/></p:cSld></p:sld>')
class Guards(unittest.TestCase):
 def test_dimension_rejection_and_sources_unchanged(self):
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/x for x in ['a.pptx','b.pptx','out.pptx']];fixture(a,12192000,6858000);fixture(b,9144000,6858000)
   before=(a.read_bytes(),b.read_bytes());r=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertNotEqual(r.returncode,0);self.assertFalse(o.exists());self.assertEqual(before,(a.read_bytes(),b.read_bytes()))
   message=r.stderr.decode()
   self.assertIn('no tenen proporció 16:9',message)
   self.assertIn('Fitxer 2: «b.pptx»',message)
   self.assertNotIn('Fitxer 1:',message)
   self.assertIn('25,40 × 19,05 cm',message)
 def test_alt_text_whitespace_survives_normalization(self):
  import xml.etree.ElementTree as E
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/n for n in ['a.pptx','b.pptx','out.pptx']];fixture(a,9144000,5143500);fixture(b)
   with zipfile.ZipFile(a) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['ppt/slides/slide1.xml']=parts['ppt/slides/slide1.xml'].replace(b'<p:spTree/>',b'<p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="shape" descr="line one&#10;line two&#9;tab&#13;return"/></p:nvSpPr></p:sp></p:spTree>')
   with zipfile.ZipFile(a,'w') as z:
    for n,v in parts.items():z.writestr(n,v)
   run=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertEqual(run.returncode,0,run.stderr.decode())
   with zipfile.ZipFile(o) as z:
    root=E.fromstring(z.read('ppt/slides/slide1.xml'))
    prop=next(e for e in root.iter() if e.tag.endswith('}cNvPr'))
    self.assertEqual(prop.get('descr'),'line one\nline two\ttab\rreturn')
 def test_secondary_embedded_fonts_are_registered(self):
  import xml.etree.ElementTree as E
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/n for n in ['a.pptx','b.pptx','out.pptx']];fixture(a);fixture(b)
   with zipfile.ZipFile(b) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['ppt/presentation.xml']=parts['ppt/presentation.xml'].replace(b'</p:presentation>',b'<p:embeddedFontLst><p:embeddedFont><p:font typeface="Test Embedded"/><p:regular r:id="fontRel"/></p:embeddedFont></p:embeddedFontLst></p:presentation>')
   parts['ppt/_rels/presentation.xml.rels']=parts['ppt/_rels/presentation.xml.rels'].replace(b'</Relationships>',b'<Relationship Id="fontRel" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/font1.fntdata"/></Relationships>')
   parts['[Content_Types].xml']=parts['[Content_Types].xml'].replace(b'</Types>',b'<Default Extension="fntdata" ContentType="application/x-fontdata"/></Types>')
   parts['ppt/fonts/font1.fntdata']=b'test-embedded-font-payload'
   with zipfile.ZipFile(b,'w') as z:
    for n,v in parts.items():z.writestr(n,v)
   run=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertEqual(run.returncode,0,run.stderr.decode())
   with zipfile.ZipFile(o) as z:
    pns='http://schemas.openxmlformats.org/presentationml/2006/main';rns='http://schemas.openxmlformats.org/officeDocument/2006/relationships'
    pres=E.fromstring(z.read('ppt/presentation.xml'));fonts=pres.find('{'+pns+'}embeddedFontLst')
    self.assertIsNotNone(fonts)
    font=fonts.find('{'+pns+'}embeddedFont');self.assertEqual(font.find('{'+pns+'}font').get('typeface'),'Test Embedded')
    rid=font.find('{'+pns+'}regular').get('{'+rns+'}id')
    rels=E.fromstring(z.read('ppt/_rels/presentation.xml.rels'))
    rel=next(x for x in rels if x.get('Id')==rid)
    self.assertEqual(z.read(rel.get('Target').lstrip('/')),b'test-embedded-font-payload')
 def test_different_physical_sizes_same_widescreen(self):
  import xml.etree.ElementTree as E, json
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/x for x in ['small.pptx','large.pptx','out.pptx']]
   fixture(a,9144000,5143500);fixture(b,18288000,10287000)
   slide='''<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="1828800" y="1028700"/><a:ext cx="9144000" cy="5143500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr sz="3600"/><a:t>Text editable</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:transition advTm="7000"><p:fade/></p:transition></p:sld>'''
   with zipfile.ZipFile(b) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['ppt/slides/slide1.xml']=slide.encode()
   with zipfile.ZipFile(b,'w') as z:
    for n,content in parts.items():z.writestr(n,content)
   before=(a.read_bytes(),b.read_bytes())
   result=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertEqual(result.returncode,0,result.stderr.decode())
   self.assertEqual(before,(a.read_bytes(),b.read_bytes()))
   ns={'p':'http://schemas.openxmlformats.org/presentationml/2006/main','a':'http://schemas.openxmlformats.org/drawingml/2006/main'}
   with zipfile.ZipFile(o) as z:
    pres=E.fromstring(z.read('ppt/presentation.xml'))
    self.assertEqual(pres.find('p:sldSz',ns).attrib,{'cx':'18288000','cy':'10287000'})
    out=E.fromstring(z.read('pptMerger/source1/ppt/slides/slide1.xml'))
    self.assertEqual(out.find('.//a:off',ns).attrib,{'x':'1828800','y':'1028700'})
    self.assertEqual(out.find('.//a:ext',ns).attrib,{'cx':'9144000','cy':'5143500'})
    self.assertEqual(out.find('.//a:rPr',ns).attrib['sz'],'3600')
    self.assertEqual(out.find('.//a:t',ns).text,'Text editable')
    self.assertEqual(out.find('p:transition',ns).attrib['advTm'],'7000')
   report=json.loads(Path(str(o)+'.report.json').read_text())
   self.assertEqual(report['totalSlides'],2)
   self.assertEqual(sum('→' in w for w in report['warnings']),1)
   from validate import validate
   self.assertFalse(validate(o,[a,b]))
 def test_uppercase_media_extension_preserves_bytes_and_relationship(self):
  import xml.etree.ElementTree as E,base64
  from validate import validate
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/x for x in ['a.pptx','b.pptx','out.pptx']];fixture(a);fixture(b)
   image=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=')
   with zipfile.ZipFile(b) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['[Content_Types].xml']=parts['[Content_Types].xml'].replace(b'</Types>',b'<Default Extension="png" ContentType="image/png"/></Types>')
   parts['ppt/media/image32.PNG']=image
   parts['ppt/slides/_rels/slide1.xml.rels']=b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image32.PNG"/></Relationships>'
   with zipfile.ZipFile(b,'w') as z:
    for n,content in parts.items():z.writestr(n,content)
   before=b.read_bytes()
   result=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertEqual(result.returncode,0,result.stderr.decode());self.assertEqual(before,b.read_bytes())
   with zipfile.ZipFile(o) as z:
    self.assertEqual(z.read('pptMerger/source1/ppt/media/image32.PNG'),image)
    types=E.fromstring(z.read('[Content_Types].xml'))
    self.assertEqual(next(e.attrib['ContentType'] for e in types if e.attrib.get('PartName')=='/pptMerger/source1/ppt/media/image32.PNG'),'image/png')
    rel=E.fromstring(z.read('pptMerger/source1/ppt/slides/_rels/slide1.xml.rels'))
    self.assertEqual(rel[0].attrib['Target'],'../media/image32.PNG')
   self.assertFalse(validate(o,[a,b]))
 def test_check_detects_lost_media_and_wrong_resource_link(self):
  import json
  with tempfile.TemporaryDirectory() as d:
   a,b,o,bad=[Path(d)/x for x in ['a.pptx','media.pptx','out.pptx','bad.pptx']];fixture(a);fixture(b)
   with zipfile.ZipFile(b) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['[Content_Types].xml']=parts['[Content_Types].xml'].replace(b'</Types>',b'<Default Extension="mp4" ContentType="video/mp4"/></Types>')
   parts['ppt/media/video1.mp4']=b'media-one';parts['ppt/media/video2.mp4']=b'media-two'
   parts['ppt/slides/_rels/slide1.xml.rels']=b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rVideo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/video" Target="../media/video1.mp4"/></Relationships>'
   with zipfile.ZipFile(b,'w') as z:
    for n,content in parts.items():z.writestr(n,content)
   r=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True);self.assertEqual(r.returncode,0,r.stderr.decode())
   with zipfile.ZipFile(o) as z:merged={n:z.read(n) for n in z.namelist()}
   for kind in ['missing','modified','wrong_link']:
    with self.subTest(kind=kind):
     with zipfile.ZipFile(bad,'w') as z:
      for n,content in merged.items():
       if kind=='missing' and n=='pptMerger/source1/ppt/media/video1.mp4':continue
       if kind=='modified' and n=='pptMerger/source1/ppt/media/video1.mp4':content=b'changed'
       if kind=='wrong_link' and n=='pptMerger/source1/ppt/slides/_rels/slide1.xml.rels':content=content.replace(b'video1.mp4',b'video2.mp4')
       z.writestr(n,content)
     r=subprocess.run([str(BIN),'--verify',str(bad),str(a),str(b)],capture_output=True)
     self.assertEqual(r.returncode,2,r.stderr.decode());checks=json.loads(r.stdout)
     self.assertTrue(checks[1]['issues']);self.assertFalse(checks[0]['issues'])
 def test_fit_non169_requires_explicit_choice_and_does_not_enlarge(self):
  import xml.etree.ElementTree as E,json
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/x for x in ['wide.pptx','fourthree.pptx','out.pptx']]
   fixture(a);fixture(b,9144000,6858000)
   with zipfile.ZipFile(b) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['ppt/slides/slide1.xml']=b'<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="6858000"/></a:xfrm></p:spPr></p:sp></p:spTree></p:cSld></p:sld>'
   with zipfile.ZipFile(b,'w') as z:
    for n,content in parts.items():z.writestr(n,content)
   before=b.read_bytes()
   r=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertNotEqual(r.returncode,0);self.assertFalse(o.exists())
   r=subprocess.run([str(BIN),'--merge','--fit-non169',str(o),str(a),str(b)],capture_output=True)
   self.assertEqual(r.returncode,0,r.stderr.decode());self.assertEqual(before,b.read_bytes())
   with zipfile.ZipFile(o) as z:
    node=E.fromstring(z.read('pptMerger/source1/ppt/slides/slide1.xml'))
    ns={'a':'http://schemas.openxmlformats.org/drawingml/2006/main'}
    self.assertEqual(node.find('.//a:off',ns).attrib,{'x':'1524000','y':'0'})
    self.assertEqual(node.find('.//a:ext',ns).attrib,{'cx':'9144000','cy':'6858000'})
   report=json.loads(Path(str(o)+'.report.json').read_text())
   self.assertTrue(all(not f['issues'] for f in report['validation']))
 def test_check_detects_missing_or_changed_animation(self):
  import json
  with tempfile.TemporaryDirectory() as d:
   a,b,o,bad=[Path(d)/x for x in ['a.pptx','animated.pptx','out.pptx','bad.pptx']];fixture(a);fixture(b)
   timing=b'<p:timing><p:tnLst><p:par><p:cTn id="1" dur="3000"/></p:par></p:tnLst></p:timing>'
   with zipfile.ZipFile(b) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['ppt/slides/slide1.xml']=parts['ppt/slides/slide1.xml'].replace(b'</p:sld>',timing+b'</p:sld>')
   with zipfile.ZipFile(b,'w') as z:
    for n,content in parts.items():z.writestr(n,content)
   r=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertEqual(r.returncode,0,r.stderr.decode())
   report=json.loads(Path(str(o)+'.report.json').read_text());self.assertEqual(report['validation'][1]['slidesWithAnimation'],1)
   with zipfile.ZipFile(o) as z:merged={n:z.read(n) for n in z.namelist()}
   for replacement in [b'',timing.replace(b'3000',b'5000')]:
    with zipfile.ZipFile(bad,'w') as z:
     for n,content in merged.items():z.writestr(n,content.replace(timing,replacement) if n=='pptMerger/source1/ppt/slides/slide1.xml' else content)
    r=subprocess.run([str(BIN),'--verify',str(bad),str(a),str(b)],capture_output=True)
    self.assertEqual(r.returncode,2,r.stderr.decode());checks=json.loads(r.stdout)
    self.assertEqual(checks[1]['status'],'import_failed_validation')
    self.assertTrue(any('animació/temporització' in issue and 'diapositiva 1' in issue for issue in checks[1]['issues']))
 def test_office_preparation_and_semantic_verifier(self):
  import json
  with tempfile.TemporaryDirectory() as d:
   a,b,o,prepared=[Path(d)/x for x in ['a.pptx','b.pptx','out.pptx','prepared']];fixture(a,9144000,5143500);fixture(b,18288000,10287000)
   r=subprocess.run([str(BIN),'--office-prepare',str(prepared),str(a),str(b)],capture_output=True)
   self.assertEqual(r.returncode,0,r.stderr.decode());meta=json.loads(r.stdout)
   self.assertEqual((meta['width'],meta['height']),(18288000,10287000));self.assertEqual([x['slides'] for x in meta['files']],[1,1])
   paths=[x['path'] for x in meta['files']]
   r=subprocess.run([str(BIN),'--office-verify',paths[0],paths[0]],capture_output=True)
   self.assertEqual(r.returncode,0,r.stderr.decode())
   r=subprocess.run([str(BIN),'--merge',str(o)]+paths,capture_output=True);self.assertEqual(r.returncode,0,r.stderr.decode())
   r=subprocess.run([str(BIN),'--office-verify',str(o)]+paths,capture_output=True)
   self.assertEqual(r.returncode,0,r.stderr.decode());self.assertEqual(len(json.loads(r.stdout)),2)
 def test_existing_output_untouched(self):
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/x for x in ['a.pptx','b.pptx','out.pptx']];fixture(a);fixture(b);o.write_bytes(b'keep')
   r=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertNotEqual(r.returncode,0);self.assertEqual(o.read_bytes(),b'keep')
 def test_two_presentations(self):
  from validate import validate
  with tempfile.TemporaryDirectory() as d:
   a,b,o=[Path(d)/x for x in ['a.pptx','b.pptx','out.pptx']];fixture(a);fixture(b)
   r=subprocess.run([str(BIN),'--merge',str(o),str(a),str(b)],capture_output=True)
   self.assertEqual(r.returncode,0,r.stderr.decode());self.assertFalse(validate(o,[a,b]))
if __name__=='__main__':unittest.main()

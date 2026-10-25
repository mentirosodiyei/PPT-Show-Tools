import importlib.util, tempfile, unittest, zipfile
from pathlib import Path
spec=importlib.util.spec_from_file_location('pptx_audit',Path(__file__).resolve().parents[1]/'tools/pptx_audit.py')
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
P=audit.P;R=audit.R;K=audit.PKG_R

def fixture(path,prefix='ppt/',missing_media=False,timing=True):
 def rel(items):return '<Relationships xmlns="'+K+'">'+''.join(f'<Relationship Id="{i}" Type="{R}/{t}" Target="{dest}"/>' for i,t,dest in items)+'</Relationships>'
 slide=prefix+'slides/slide1.xml';layout=prefix+'slideLayouts/slideLayout1.xml';master=prefix+'slideMasters/slideMaster1.xml';media=prefix+'media/image1.png'
 overrides={'ppt/presentation.xml':'presentationml.presentation.main+xml',slide:'presentationml.slide+xml',layout:'presentationml.slideLayout+xml',master:'presentationml.slideMaster+xml'}
 parts={'[Content_Types].xml':'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'+''.join(f'<Override PartName="/{n}" ContentType="application/vnd.openxmlformats-officedocument.{t}"/>' for n,t in overrides.items())+'</Types>',
 '_rels/.rels':rel([('r1','officeDocument','ppt/presentation.xml')]),
 'ppt/presentation.xml':f'<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="256" r:id="r1"/></p:sldIdLst></p:presentation>',
 'ppt/_rels/presentation.xml.rels':rel([('r1','slide','/'+slide)]),
 slide:f'<p:sld xmlns:p="{P}">'+('<p:timing/>' if timing else '')+'</p:sld>',
 prefix+'slides/_rels/slide1.xml.rels':rel([('r1','slideLayout','/'+layout),('r2','image','/'+media)]),
 layout:f'<p:sldLayout xmlns:p="{P}"/>',
 prefix+'slideLayouts/_rels/slideLayout1.xml.rels':rel([('r1','slideMaster','/'+master)]),
 master:f'<p:sldMaster xmlns:p="{P}"/>'}
 if not missing_media:parts[media]=b'example-binary'
 with zipfile.ZipFile(path,'w') as z:
  for n,v in parts.items():z.writestr(n,v)

class AuditRegression(unittest.TestCase):
 def test_relocated_parts_and_real_losses(self):
  with tempfile.TemporaryDirectory() as d:
   a,b=Path(d)/'a.pptx',Path(d)/'b.pptx';fixture(a)
   original=audit.inspect_deck(a)
   fixture(b,'pptMerger/source7/ppt/')
   result=audit.inspect_deck(b)
   self.assertEqual(result['master_count'],1)
   self.assertEqual(result['layout_count'],1)
   self.assertEqual(audit.compare([original],result)['structural_gate'],'PASS')
   fixture(b,'pptMerger/source7/ppt/',missing_media=True)
   q=audit.compare([original],audit.inspect_deck(b))
   self.assertEqual(q['structural_gate'],'FAIL');self.assertTrue(q['missing_binary_hashes'])
   fixture(b,'pptMerger/source7/ppt/',timing=False)
   q=audit.compare([original],audit.inspect_deck(b))
   self.assertEqual(q['structural_gate'],'REVIEW');self.assertTrue(any('timings' in w for w in q['warnings']))
 def test_changed_timing_with_same_count_requires_review(self):
  with tempfile.TemporaryDirectory() as d:
   a,b=Path(d)/'a.pptx',Path(d)/'b.pptx';fixture(a);fixture(b)
   with zipfile.ZipFile(b) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['ppt/slides/slide1.xml']=parts['ppt/slides/slide1.xml'].replace(b'<p:timing/>',b'<p:timing dur="999"/>')
   with zipfile.ZipFile(b,'w') as z:
    for n,v in parts.items():z.writestr(n,v)
   before,after=audit.inspect_deck(a),audit.inspect_deck(b)
   self.assertEqual(before['timings'],after['timings'])
   q=audit.compare([before],after)
   self.assertEqual(q['structural_gate'],'REVIEW')
   self.assertTrue(any('Per-slide timing' in w for w in q['warnings']))
 def test_copied_but_unregistered_font_fails(self):
  with tempfile.TemporaryDirectory() as d:
   a,b=Path(d)/'a.pptx',Path(d)/'b.pptx';fixture(a)
   with zipfile.ZipFile(a) as z:parts={n:z.read(n) for n in z.namelist()}
   parts['ppt/fonts/font1.fntdata']=b'font-data'
   parts['ppt/_rels/presentation.xml.rels']=parts['ppt/_rels/presentation.xml.rels'].replace(b'</Relationships>',f'<Relationship Id="f1" Type="{R}/font" Target="fonts/font1.fntdata"/></Relationships>'.encode())
   parts['ppt/presentation.xml']=parts['ppt/presentation.xml'].replace(b'</p:presentation>',b'<p:embeddedFontLst><p:embeddedFont><p:font typeface="Example"/><p:regular r:id="f1"/></p:embeddedFont></p:embeddedFontLst></p:presentation>')
   with zipfile.ZipFile(a,'w') as z:
    for n,v in parts.items():z.writestr(n,v)
   parts['ppt/presentation.xml']=parts['ppt/presentation.xml'].replace(b'<p:regular r:id="f1"/>',b'')
   with zipfile.ZipFile(b,'w') as z:
    for n,v in parts.items():z.writestr(n,v)
   q=audit.compare([audit.inspect_deck(a)],audit.inspect_deck(b))
   self.assertEqual(q['structural_gate'],'FAIL')
   self.assertFalse(q['missing_binary_hashes'])
   self.assertTrue(any('font payloads' in e for e in q['errors']))
 def test_unreadable_inputs_never_pass(self):
  with tempfile.TemporaryDirectory() as d:
   missing=audit.inspect_deck(Path(d)/'absent.pptx')
   self.assertEqual(audit.compare([missing],missing)['structural_gate'],'FAIL')
 def test_notes_master_change_requires_review(self):
  with tempfile.TemporaryDirectory() as d:
   paths=[Path(d)/'a.pptx',Path(d)/'b.pptx']
   for path,label in zip(paths,['Original notes style','Different notes style']):
    fixture(path)
    with zipfile.ZipFile(path) as z:parts={n:z.read(n) for n in z.namelist()}
    relpath='ppt/slides/_rels/slide1.xml.rels'
    parts[relpath]=parts[relpath].replace(b'</Relationships>',f'<Relationship Id="notes" Type="{R}/notesSlide" Target="../notesSlides/n.xml"/></Relationships>'.encode())
    parts['ppt/notesSlides/n.xml']=f'<p:notes xmlns:p="{P}"/>'.encode()
    parts['ppt/notesSlides/_rels/n.xml.rels']=f'<Relationships xmlns="{K}"><Relationship Id="nm" Type="{R}/notesMaster" Target="../notesMasters/m.xml"/></Relationships>'.encode()
    parts['ppt/notesMasters/m.xml']=f'<p:notesMaster xmlns:p="{P}" name="{label}"/>'.encode()
    with zipfile.ZipFile(path,'w') as z:
     for n,v in parts.items():z.writestr(n,v)
   q=audit.compare([audit.inspect_deck(paths[0])],audit.inspect_deck(paths[1]))
   self.assertEqual(q['structural_gate'],'REVIEW')
   self.assertTrue(any('notes master' in w for w in q['warnings']))

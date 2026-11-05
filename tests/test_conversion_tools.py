import json, subprocess, tempfile, unittest, zipfile, hashlib, sys
from pathlib import Path
import xml.etree.ElementTree as E
from test_guards import fixture, BIN
P='http://schemas.openxmlformats.org/presentationml/2006/main';R='http://schemas.openxmlformats.org/officeDocument/2006/relationships';A='http://schemas.openxmlformats.org/drawingml/2006/main';K='http://schemas.openxmlformats.org/package/2006/relationships'

def notes_fixture(path):
 fixture(path,9144000,6858000)
 with zipfile.ZipFile(path) as z:parts={n:z.read(n) for n in z.namelist()}
 pres=E.fromstring(parts['ppt/presentation.xml']);lst=pres.find('{'+P+'}sldIdLst');lst.clear()
 rels=E.fromstring(parts['ppt/_rels/presentation.xml.rels']);rels.clear()
 cts=E.fromstring(parts['[Content_Types].xml']);ctns='http://schemas.openxmlformats.org/package/2006/content-types'
 for i in range(1,4):
  E.SubElement(lst,'{'+P+'}sldId',{'id':str(255+i),'{'+R+'}id':'r'+str(i)})
  E.SubElement(rels,'{'+K+'}Relationship',{'Id':'r'+str(i),'Type':R+'/slide','Target':f'slides/slide{i}.xml'})
  parts[f'ppt/slides/slide{i}.xml']=parts['ppt/slides/slide1.xml']
  if i>1:E.SubElement(cts,'{'+ctns+'}Override',{'PartName':f'/ppt/slides/slide{i}.xml','ContentType':'application/vnd.openxmlformats-officedocument.presentationml.slide+xml'})
  if i<3:
   text='Primera nota &amp; comprovació. Àudio, vídeo i acció.' if i==1 else ' '.join(['Notes llargues per comprovar que tot el text queda visible.']*100)
   parts[f'ppt/slides/_rels/slide{i}.xml.rels']=f'<Relationships xmlns="{K}"><Relationship Id="n" Type="{R}/notesSlide" Target="../notesSlides/notesSlide{i}.xml"/></Relationships>'.encode()
   parts[f'ppt/notesSlides/notesSlide{i}.xml']=f'<p:notes xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Body"/><p:cNvSpPr/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>{text}</a:t></a:r><a:br/><a:r><a:t>Segona línia.</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Number"/><p:cNvSpPr/><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:bodyPr/><a:p><a:fld id="n"><a:t>999</a:t></a:fld></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>'.encode()
   E.SubElement(cts,'{'+ctns+'}Override',{'PartName':f'/ppt/notesSlides/notesSlide{i}.xml','ContentType':'application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml'})
 parts['ppt/presentation.xml']=E.tostring(pres);parts['ppt/_rels/presentation.xml.rels']=E.tostring(rels);parts['[Content_Types].xml']=E.tostring(cts)
 with zipfile.ZipFile(path,'w') as z:
  for n,v in parts.items():z.writestr(n,v)

class ConversionToolsTests(unittest.TestCase):
 def test_notes_mapping_fit_blank_and_text(self):
  with tempfile.TemporaryDirectory() as d:
   a,o=Path(d)/'notes.pptx',Path(d)/'out.pptx';notes_fixture(a);original=a.read_bytes()
   r=subprocess.run([str(BIN),'--extract-notes',str(o),str(a)],capture_output=True,text=True)
   self.assertEqual(r.returncode,0,r.stderr)
   report=json.loads(r.stdout);self.assertEqual(len(report['pages']),3)
   self.assertGreater(report['pages'][0]['fontSize'],report['pages'][1]['fontSize']);self.assertTrue(report['pages'][2]['empty']);self.assertTrue(Path(str(o)+'.report.log').exists())
   with zipfile.ZipFile(o) as z:
    for i in range(1,4):
     root=E.fromstring(z.read(f'ppt/slides/slide{i}.xml'))
     self.assertEqual(root.find('.//{'+A+'}srgbClr').get('val'),'000000')
     ts=[t.text or '' for t in root.iter('{'+A+'}t')]
     if i==1:self.assertEqual(ts,['Primera nota & comprovació. Àudio, vídeo i acció.','Segona línia.'])
     if i==3:
      self.assertEqual(ts,['No Notes on this slide'])
      self.assertEqual(root.find('.//{'+A+'}rPr').get('sz'),'2400')
      self.assertEqual(root.find('.//{'+A+'}pPr').get('algn'),'ctr')
     self.assertNotIn('999',ts)
   self.assertEqual(original,a.read_bytes())
   again=subprocess.run([str(BIN),'--extract-notes',str(o),str(a)],capture_output=True)
   self.assertNotEqual(again.returncode,0)
 def test_single_4_3_keeps_content_and_notes(self):
  with tempfile.TemporaryDirectory() as d:
   a,o=Path(d)/'original.pptx',Path(d)/'wide.pptx';notes_fixture(a);before=a.read_bytes()
   r=subprocess.run([str(BIN),'--to-169',str(o),str(a)],capture_output=True,text=True)
   self.assertEqual(r.returncode,0,r.stderr)
   with zipfile.ZipFile(o) as z,zipfile.ZipFile(a) as source:
    pres=E.fromstring(z.read('ppt/presentation.xml'));size=pres.find('{'+P+'}sldSz');self.assertEqual(int(size.get('cx'))*9,int(size.get('cy'))*16)
    self.assertEqual(z.read('ppt/notesSlides/notesSlide1.xml'),source.read('ppt/notesSlides/notesSlide1.xml'))
   self.assertEqual(before,a.read_bytes())
 def test_valid_pdf_page_order_rotation_and_sizes(self):
  import struct
  with tempfile.TemporaryDirectory() as d:
   a,o=Path(d)/'pages.pdf',Path(d)/'out.pptx'
   objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>']
   for size,rotation,stream in [('960 540',0,6),('540 960',0,7),('960 540',90,8)]:
    annotation='/Annots [9 0 R]' if stream==6 else ''
    objects.append(f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {size}] /Rotate {rotation} /Resources << >> /Contents {stream} 0 R {annotation} >>'.encode())
   for color in ['1 0 0','0 1 0','0 0 1']:
    data=f'{color} rg 30 30 100 100 re f'.encode();objects.append(f'<< /Length {len(data)} >>\nstream\n'.encode()+data+b'\nendstream')
   objects.append(b'<< /Type /Annot /Subtype /Text /Rect [0 0 20 20] /Contents (Review annotation) >>')
   pdf=b'%PDF-1.4\n';offsets=[0]
   for i,obj in enumerate(objects,1):offsets.append(len(pdf));pdf+=f'{i} 0 obj\n'.encode()+obj+b'\nendobj\n'
   xref=len(pdf);pdf+=f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode()
   for offset in offsets[1:]:pdf+=f'{offset:010} 00000 n \n'.encode()
   pdf+=f'trailer << /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF'.encode();a.write_bytes(pdf)
   r=subprocess.run([str(BIN),'--pdf-to-ppt',str(o),str(a)],capture_output=True,text=True)
   self.assertEqual(r.returncode,0,r.stderr);self.assertEqual(len(json.loads(r.stdout)['pages']),3);self.assertTrue(json.loads(r.stdout)['pages'][0]['warnings'])
   with zipfile.ZipFile(o) as z:
    dims=[struct.unpack('>II',z.read(f'ppt/media/page{i}.png')[16:24]) for i in range(1,4)]
    self.assertEqual(dims,[(2560,1440),(1440,2560),(1440,2560)])
    # Verify actual rendering placement, not just image dimensions and slide count.
    for number,x,y,color in [(1,150,150,(255,0,0)),(3,150,2410,(0,0,255))]:
     png,bmp=Path(d)/'page.png',Path(d)/'page.bmp';png.write_bytes(z.read(f'ppt/media/page{number}.png'))
     subprocess.run(['sips','-s','format','bmp',str(png),'--out',str(bmp)],check=True,capture_output=True)
     data=bmp.read_bytes();offset=struct.unpack_from('<I',data,10)[0];width,height=struct.unpack_from('<ii',data,18);bits=struct.unpack_from('<H',data,28)[0]
     self.assertIn(bits,[24,32]);stride=((width*bits+31)//32)*4
     row=y if height>0 else abs(height)-1-y;index=offset+row*stride+x*(bits//8)
     blue,green,red=data[index:index+3]
     self.assertTrue(all(abs(a-b)<12 for a,b in zip((red,green,blue),color)),(number,red,green,blue))

   self.assertEqual(a.read_bytes(),pdf)
 def test_bad_pdf_reports_error_and_log(self):
  with tempfile.TemporaryDirectory() as d:
   a,o=Path(d)/'bad.pdf',Path(d)/'out.pptx';a.write_bytes(b'not a pdf')
   r=subprocess.run([str(BIN),'--pdf-to-ppt',str(o),str(a)],capture_output=True,text=True)
   self.assertNotEqual(r.returncode,0);self.assertFalse(o.exists());self.assertTrue(list(Path(d).glob('Deckwright-ERROR-*.log')))
if __name__=='__main__':unittest.main()

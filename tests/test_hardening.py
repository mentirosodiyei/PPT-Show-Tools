"""Regression tests for 0.5.3: untrusted packages, PowerPoint limits and staged outputs."""
import os, stat, subprocess, tempfile, unittest, zipfile
from pathlib import Path
import xml.etree.ElementTree as E
from test_guards import fixture, BIN
from test_conversion_tools import notes_fixture, P, A, R, K

def with_symlink(source, path, target):
 with zipfile.ZipFile(source) as z:parts={n:z.read(n) for n in z.namelist()}
 with zipfile.ZipFile(path,'w') as z:
  for n,v in parts.items():z.writestr(n,v)
  link=zipfile.ZipInfo('ppt/media/leak.png');link.create_system=3;link.external_attr=(stat.S_IFLNK|0o777)<<16
  z.writestr(link,str(target))

def tiny_pdf(path, width, height):
 objects=[b'<< /Type /Catalog /Pages 2 0 R >>',b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << >> >>'.encode()]
 pdf=b'%PDF-1.4\n';offsets=[]
 for i,obj in enumerate(objects,1):offsets.append(len(pdf));pdf+=f'{i} 0 obj\n'.encode()+obj+b'\nendobj\n'
 xref=len(pdf);pdf+=f'xref\n0 {len(objects)+1}\n0000000000 65535 f \n'.encode()+b''.join(f'{o:010} 00000 n \n'.encode() for o in offsets)
 pdf+=f'trailer << /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF'.encode();path.write_bytes(pdf)

class Hardening(unittest.TestCase):
 def test_symlinked_parts_are_rejected_everywhere(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);secret=d/'secret.txt';secret.write_text('SECRET');a,b,evil=d/'a.pptx',d/'b.pptx',d/'evil.pptx'
   fixture(a);fixture(b,9144000,6858000);with_symlink(b,evil,secret)
   for args,out in [(['--merge',str(d/'m.pptx'),str(a),str(evil)],d/'m.pptx'),(['--to-169',str(d/'w.pptx'),str(evil)],d/'w.pptx'),(['--extract-notes',str(d/'n.pptx'),str(evil)],d/'n.pptx')]:
    r=subprocess.run([str(BIN)]+args,capture_output=True,text=True)
    self.assertNotEqual(r.returncode,0,args);self.assertIn('enllaços simbòlics',r.stderr);self.assertFalse(out.exists())
 def test_non_zip_input_is_rejected_cleanly(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);bad=d/'bad.pptx';bad.write_bytes(b'not a zip');a=d/'a.pptx';fixture(a)
   r=subprocess.run([str(BIN),'--merge',str(d/'o.pptx'),str(a),str(bad)],capture_output=True,text=True)
   self.assertEqual(r.returncode,1);self.assertIn('no és un paquet ZIP',r.stderr)
 def test_already_widescreen_is_not_converted(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);a=d/'wide.pptx';fixture(a)
   r=subprocess.run([str(BIN),'--to-169',str(d/'o.pptx'),str(a)],capture_output=True,text=True)
   self.assertNotEqual(r.returncode,0);self.assertIn('ja és 16:9',r.stderr);self.assertFalse((d/'o.pptx').exists())
 def test_very_tall_pdf_stays_within_powerpoint_limits(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);a,o=d/'tall.pdf',d/'o.pptx';tiny_pdf(a,200,2000)
   r=subprocess.run([str(BIN),'--pdf-to-ppt',str(o),str(a)],capture_output=True,text=True)
   self.assertEqual(r.returncode,0,r.stderr)
   with zipfile.ZipFile(o) as z:size=E.fromstring(z.read('ppt/presentation.xml')).find('{'+P+'}sldSz')
   for key in ('cx','cy'):self.assertTrue(914400<=int(size.get(key))<=51206400,size.attrib)
   self.assertEqual(int(size.get('cy')),51206400)
 def test_pdf_too_extreme_for_powerpoint_fails_without_output(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);a,o=d/'strip.pdf',d/'o.pptx';tiny_pdf(a,5000,20)
   r=subprocess.run([str(BIN),'--pdf-to-ppt',str(o),str(a)],capture_output=True,text=True)
   self.assertNotEqual(r.returncode,0);self.assertFalse(o.exists());self.assertFalse(Path(str(o)+'.report.json').exists())
 def test_unfittable_notes_fail_instead_of_invalid_font_size(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);a,o=d/'n.pptx',d/'o.pptx';notes_fixture(a)
   with zipfile.ZipFile(a) as z:parts={n:z.read(n) for n in z.namelist()}
   huge=' '.join(['Text extremadament llarg per a una sola pàgina.']*40000)
   parts['ppt/notesSlides/notesSlide2.xml']=parts['ppt/notesSlides/notesSlide2.xml'].replace(b'Segona l\xc3\xadnia.',huge.encode())
   with zipfile.ZipFile(a,'w') as z:
    for n,v in parts.items():z.writestr(n,v)
   r=subprocess.run([str(BIN),'--extract-notes',str(o),str(a)],capture_output=True,text=True)
   self.assertNotEqual(r.returncode,0);self.assertIn('massa llargues',r.stderr);self.assertFalse(o.exists())
 def test_existing_report_log_is_never_overwritten(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);a,o=d/'n.pptx',d/'o.pptx';notes_fixture(a);log=Path(str(o)+'.report.log');log.write_text('keep')
   r=subprocess.run([str(BIN),'--extract-notes',str(o),str(a)],capture_output=True,text=True)
   self.assertNotEqual(r.returncode,0);self.assertEqual(log.read_text(),'keep');self.assertFalse(o.exists())
 def test_notes_font_sizes_are_valid_drawingml(self):
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);a,o=d/'n.pptx',d/'o.pptx';notes_fixture(a)
   r=subprocess.run([str(BIN),'--extract-notes',str(o),str(a)],capture_output=True,text=True);self.assertEqual(r.returncode,0,r.stderr)
   with zipfile.ZipFile(o) as z:
    for n in z.namelist():
     if n.startswith('ppt/slides/slide'):
      for e in E.fromstring(z.read(n)).iter():
       if e.get('sz'):self.assertTrue(100<=int(e.get('sz'))<=400000,(n,e.get('sz')))
if __name__=='__main__':unittest.main()

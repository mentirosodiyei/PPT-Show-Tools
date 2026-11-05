"""Exercise the actual verifier used after each native paste, with corrupt outputs."""
import json, subprocess, tempfile, unittest, zipfile
from pathlib import Path
from test_guards import fixture, BIN

class NativeCheckpoint(unittest.TestCase):
 def test_equal_count_wrong_source_and_lost_effects_are_rejected(self):
  with tempfile.TemporaryDirectory() as d:
   a,b,out=[Path(d)/n for n in ('a.pptx','b.pptx','out.pptx')]
   for path,label in ((a,'First presentation'),(b,'Second presentation')):
    fixture(path)
    with zipfile.ZipFile(path) as z: parts={n:z.read(n) for n in z.namelist()}
    parts['ppt/slides/slide1.xml']=f'''<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{label}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:transition advTm="7000"><p:fade/></p:transition><p:timing><p:tnLst><p:par><p:cTn id="1" dur="1000"/></p:par></p:tnLst></p:timing></p:sld>'''.encode()
    with zipfile.ZipFile(path,'w') as z:
     for n,data in parts.items(): z.writestr(n,data)
   merge=subprocess.run([str(BIN),'--merge',str(out),str(a),str(b)],capture_output=True,text=True)
   self.assertEqual(merge.returncode,0,merge.stderr)
   def verify():
    r=subprocess.run([str(BIN),'--office-verify',str(out),str(a),str(b)],capture_output=True,text=True)
    return r,json.loads(r.stdout)
   r,_=verify();self.assertEqual(r.returncode,0,r.stdout+r.stderr)
   with zipfile.ZipFile(out) as z: merged={n:z.read(n) for n in z.namelist()}
   target='pptMerger/source1/ppt/slides/slide1.xml'
   for mode in ('wrong_source','lost_effects'):
    with self.subTest(mode=mode):
     altered=dict(merged)
     if mode=='wrong_source':
      altered[target]=altered[target].replace(b'Second presentation',b'First presentation')
     else:
      import xml.etree.ElementTree as E
      root=E.fromstring(altered[target])
      for child in list(root):
       if child.tag.rsplit('}',1)[-1] in ('timing','transition'): root.remove(child)
      altered[target]=E.tostring(root)
     with zipfile.ZipFile(out,'w') as z:
      for n,data in altered.items(): z.writestr(n,data)
     r,checks=verify();self.assertEqual(r.returncode,2)
     issues=' '.join(checks[1]['issues'])
     if mode=='wrong_source':self.assertIn('text/order',issues)
     else:
      self.assertIn('animation/timing',issues)
      self.assertIn('transition/advance',issues)

import json,subprocess,tempfile,unittest,zipfile
from pathlib import Path
from test_guards import fixture,BIN
class SemanticText(unittest.TestCase):
 def test_run_changes_and_computed_note_numbers_but_not_text_loss(self):
  with tempfile.TemporaryDirectory() as d:
   source=Path(d)/'source.pptx';output=Path(d)/'output.pptx';fixture(source)
   with zipfile.ZipFile(source) as z:parts={n:z.read(n) for n in z.namelist()}
   slide=lambda runs:('<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p>'+''.join('<a:r><a:t>'+t+'</a:t></a:r>' for t in runs)+'</a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>').encode()
   notes=lambda n,text:('<p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>'+text+'</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:fld><a:t>'+n+'</a:t></a:fld></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>').encode()
   parts['ppt/slides/slide1.xml']=slide(['Hello ','world'])
   parts['ppt/slides/_rels/slide1.xml.rels']=b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="notes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notes1.xml"/></Relationships>'
   parts['ppt/notesSlides/notes1.xml']=notes('1','Presenter notes')
   def write(p):
    with zipfile.ZipFile(p,'w') as z:
     for n,b in parts.items():z.writestr(n,b)
   write(source)
   parts['ppt/slides/slide1.xml']=slide(['Hello world']);parts['ppt/notesSlides/notes1.xml']=notes('99','Presenter notes');write(output)
   def verify():return subprocess.run([str(BIN),'--office-verify',str(output),str(source)],capture_output=True,text=True)
   result=verify();self.assertEqual(result.returncode,0,result.stderr+result.stdout)
   parts['ppt/slides/slide1.xml']=slide(['Hello']);parts['ppt/notesSlides/notes1.xml']=notes('99','Lost notes');write(output)
   result=verify();self.assertEqual(result.returncode,2)
   issues=json.loads(result.stdout)[0]['issues'];self.assertTrue(any('text/ordre' in i for i in issues));self.assertTrue(any('notes diferent' in i for i in issues))
if __name__=='__main__':unittest.main()

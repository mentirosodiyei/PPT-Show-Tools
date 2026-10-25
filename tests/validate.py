#!/usr/bin/env python3
"""Structural verification, never a substitute for PowerPoint playback."""
import sys, zipfile, posixpath, hashlib, json, urllib.parse
import xml.etree.ElementTree as E
from pathlib import Path
P='http://schemas.openxmlformats.org/presentationml/2006/main'
R='http://schemas.openxmlformats.org/officeDocument/2006/relationships'
N={'p':P,'r':R}
def digest(b):return hashlib.sha256(b).hexdigest()
def ordered(z):
 root=E.fromstring(z.read('ppt/presentation.xml'))
 rel={r.attrib['Id']:r.attrib['Target'] for r in E.fromstring(z.read('ppt/_rels/presentation.xml.rels'))}
 return [posixpath.normpath('/'+rel[s.attrib['{'+R+'}id']]).lstrip('/') if rel[s.attrib['{'+R+'}id']].startswith('/') else posixpath.normpath('ppt/'+rel[s.attrib['{'+R+'}id']]) for s in root.find('p:sldIdLst',N)]
def validate(output, inputs):
 errors=[]; counts=[]; features={'timing':0,'transition':0,'advanceTimers':0,'notes':0,'media':0,'mediaBytes':0}; checked=0; identical=0; masterRemaps=0; normalizedParts=0
 with zipfile.ZipFile(output) as out:
  assert out.testzip() is None
  names=set(out.namelist()); slides=ordered(out); expected=[]
  for i,f in enumerate(inputs):
   prefix='' if i==0 else f'pptMerger/source{i}/'
   with zipfile.ZipFile(f) as source:
    sourceSize=E.fromstring(source.read('ppt/presentation.xml')).find('p:sldSz',N).attrib
    targetSize=E.fromstring(out.read('ppt/presentation.xml')).find('p:sldSz',N).attrib
    scaled=any(sourceSize[k]!=targetSize[k] for k in ['cx','cy'])
    srcslides=ordered(source); counts.append(len(srcslides)); expected += [prefix+s for s in srcslides]
    for s in srcslides:
     b=source.read(s); node=E.fromstring(b)
     for key in ['timing','transition']:
      features[key]+=int(node.find('p:'+key,N) is not None)
     tr=node.find('p:transition',N)
     features['advanceTimers']+=int(tr is not None and 'advTm' in tr.attrib)
     if not scaled and out.read(prefix+s)!=b: errors.append('slide bytes changed: '+s)
     if scaled:
      result=E.fromstring(out.read(prefix+s))
      # Content, shape identity and timelines must survive normalization.
      if list(node.itertext())!=list(result.itertext()):errors.append('slide text changed: '+s)
      for key in ['timing','transition']:
       left,right=node.find('p:'+key,N),result.find('p:'+key,N)
       if (left is None)!=(right is None) or (left is not None and E.tostring(left)!=E.tostring(right)):errors.append('timeline changed: '+s)
      if [x.attrib for x in node.iter('{'+P+'}cNvPr')]!=[x.attrib for x in result.iter('{'+P+'}cNvPr')]:errors.append('shape identity changed: '+s)
    for name in source.namelist():
     if name.startswith('ppt/notesSlides/notesSlide') and name.endswith('.xml'):features['notes']+=1
     if name.startswith('ppt/media/') and not name.endswith('/'):
      features['media']+=1; features['mediaBytes']+=len(source.read(name))
     # Every resource except intentionally changed package bookkeeping must match.
     if name.startswith('[trash]/') or name.endswith('/') or name.endswith('.rels') or name in ['[Content_Types].xml','ppt/presentation.xml']:continue
     checked+=1
     a,b=source.read(name),out.read(prefix+name)
     if a==b:identical+=1
     if scaled and name.endswith('.xml') and any(name.startswith('ppt/'+folder+'/') for folder in ['slides','slideLayouts','slideMasters','theme','charts']):
      normalizedParts+=1
      if list(E.fromstring(a).itertext())!=list(E.fromstring(b).itertext()):errors.append('normalized text changed: '+name)
     elif i and '/slideMasters/' in name and name.endswith('.xml'):
      masterRemaps+=1
      # Layout IDs must be unique across all imported masters. No other
      # differences are permitted in these XML parts.
      a,b=E.fromstring(a),E.fromstring(b)
      for node in [a,b]:
       for layout in node.iter('{'+P+'}sldLayoutId'):layout.attrib['id']='NORMALIZED'
      if E.tostring(a)!=E.tostring(b):errors.append('master content changed: '+prefix+name)
     elif digest(a)!=digest(b):errors.append('resource changed: '+prefix+name)
  if slides!=expected:errors.append('slide order mismatch')
  parsed=0; missing=[]; external=[]
  for name in sorted(names):
   if name.endswith('.xml') or name.endswith('.rels'):
    try:node=E.fromstring(out.read(name));parsed+=1
    except E.ParseError as e:errors.append(f'XML {name}: {e}');continue
   if name.endswith('.rels'):
    ownerdir=posixpath.dirname(posixpath.dirname(name))
    if name=='_rels/.rels':ownerdir=''
    ids=set()
    for rel in node:
     if rel.attrib['Id'] in ids:errors.append('duplicate rel id: '+name)
     ids.add(rel.attrib['Id'])
     target=rel.attrib.get('Target','')
     if rel.attrib.get('TargetMode')=='External':external.append(target);continue
     target=urllib.parse.unquote(urllib.parse.urlsplit(target).path)
     target=posixpath.normpath(target.lstrip('/') if target.startswith('/') else posixpath.join(ownerdir,target))
     if target not in names:missing.append((name,target))
  errors += ['missing relationship: '+str(x) for x in missing]
  types=E.fromstring(out.read('[Content_Types].xml'))
  overrides={x.attrib['PartName'].lstrip('/') for x in types if x.tag.endswith('Override')}
  defaults={x.attrib['Extension'].lower() for x in types if x.tag.endswith('Default')}
  for name in names:
   if name.endswith('/') or name=='[Content_Types].xml':continue
   if name not in overrides and name.rsplit('.',1)[-1].lower() not in defaults:errors.append('missing content type: '+name)
 report={'inputs':counts,'outputSlides':len(slides),'orderedSlidesMatch':slides==expected,'resourcesVerified':checked,'byteIdenticalResources':identical,'mastersWithOnlyLayoutIDRemapping':masterRemaps,'normalizedXMLPartsContentChecked':normalizedParts,'normalizedRenderingValidated':False,'xmlPartsParsed':parsed,'featuresInSources':features,'externalRelationships':len(external),'errors':errors,'powerPointPlaybackValidated':False}
 print(json.dumps(report,indent=2));return bool(errors)
if __name__=='__main__':sys.exit(validate(sys.argv[1],sys.argv[2:]))

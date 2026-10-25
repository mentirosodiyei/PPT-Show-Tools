"""Read-only, strict package preflight before any size conversion."""
import collections,posixpath,zipfile
import xml.etree.ElementTree as ET

def inspect(path):
 issues=[]
 with zipfile.ZipFile(path) as z:
  names=z.namelist(); present=set(names)
  if len(names)>100000 or sum(i.file_size for i in z.infolist())>4*1024**3:raise ValueError('Paquet massa gran.')
  for n,count in collections.Counter(names).items():
   if count>1:issues.append('Entrada ZIP duplicada: '+n)
  bad=z.testzip()
  if bad:issues.append('CRC incorrecte: '+bad)
  for item in z.infolist():
   n=item.filename
   if n.startswith(('/','\\')) or '..' in n.split('/') or '\\' in n or (item.external_attr>>16)&0o170000==0o120000 or item.flag_bits&1:raise ValueError('Ruta, enllaç o xifrat no admès: '+n)
  trees={}
  for n in names:
   if n.endswith(('.xml','.rels')):
    try:trees[n]=ET.fromstring(z.read(n))
    except ET.ParseError as e:issues.append('XML incorrecte: '+n+': '+str(e))
  for n in ['[Content_Types].xml','ppt/presentation.xml','_rels/.rels']:
   if n not in trees:issues.append('Falta XML obligatori: '+n)
  ct=trees.get('[Content_Types].xml')
  if ct is not None:
   defaults={x.get('Extension','').lower() for x in ct if x.tag.endswith('}Default')}
   overrides={x.get('PartName','').lstrip('/') for x in ct if x.tag.endswith('}Override')}
   for n in names:
    if not n.endswith('/') and not n.startswith('[trash]/') and n!='[Content_Types].xml' and n not in overrides and n.rsplit('.',1)[-1].lower() not in defaults:issues.append('Falta ContentType: '+n)
  for n,t in trees.items():
   if not n.endswith('.rels'):continue
   owner='' if n=='_rels/.rels' else posixpath.dirname(posixpath.dirname(n))+'/'+posixpath.basename(n)[:-5]
   for rel in t:
    if rel.get('TargetMode')=='External':continue
    target=rel.get('Target','').split('#',1)[0]
    from urllib.parse import unquote
    target=unquote(target)
    resolved=posixpath.normpath(target.lstrip('/') if target.startswith('/') else posixpath.join(posixpath.dirname(owner),target))
    if resolved not in present:issues.append('Relació interna trencada: '+n+' → '+resolved)
 return issues

if __name__=='__main__':
 import json,sys
 failures=[]
 for name in sys.argv[1:]:
  try:issues=inspect(name)
  except Exception as e:issues=[str(e)]
  if issues:failures.append({'file':name,'issues':issues})
 print(json.dumps(failures,ensure_ascii=False))
 sys.exit(1 if failures else 0)

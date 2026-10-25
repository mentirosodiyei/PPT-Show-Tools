#!/usr/bin/env python3
"""Loopback-only Office task-pane host. No filesystem API exposed to the browser."""
import threading,functools
import argparse,datetime,shutil,base64,http.server,json,secrets,ssl,subprocess,tempfile,uuid,zipfile
from pathlib import Path
from urllib.parse import urlsplit,unquote,quote
from preflight import inspect as preflight
PORT=43187
class BridgeServer(http.server.ThreadingHTTPServer):
 daemon_threads=True
 def __init__(self,*args,**kwargs):
  self.state_lock=threading.RLock()
  super().__init__(*args,**kwargs)
def serialized(method):
 @functools.wraps(method)
 def call(self,*args,**kwargs):
  with self.server.state_lock:
   return method(self,*args,**kwargs)
 return call
class Bridge(http.server.BaseHTTPRequestHandler):
 timeout=60
 def log_message(self,*args):pass
 def answer(self,status,value,kind='application/json'):
  data=json.dumps(value,ensure_ascii=False).encode() if kind=='application/json' else value
  self.send_response(status);self.send_header('Content-Type',kind);self.send_header('Content-Length',str(len(data)));self.send_header('Cache-Control','no-store');
  if getattr(self,'downloadName',None):self.send_header('Content-Disposition',"attachment; filename*=UTF-8''"+quote(self.downloadName,safe=''))
  self.send_header('X-Content-Type-Options','nosniff');self.end_headers();self.wfile.write(data)
 def allowed(self):
  return self.headers.get('Host') in [f'localhost:{PORT}',f'127.0.0.1:{PORT}'] and self.headers.get('Origin',f'https://localhost:{PORT}')==f'https://localhost:{PORT}'
 @serialized
 def do_GET(self):
  if not self.allowed():return self.answer(403,{'error':'Origin no permès'})
  path=urlsplit(self.path).path
  if path=='/config':
   # The token is only for the task pane itself, never for a page from another site.
   if self.headers.get('Sec-Fetch-Site','same-origin') not in ['same-origin','none']:return self.answer(403,{'error':'Origin no permès'})
   return self.answer(200,{'token':self.server.token,'job':self.server.job})
  assets={'/':'index.html','/index.html':'index.html','/taskpane.js':'taskpane.js','/merge.js':'merge.js','/style.css':'style.css','/icon.png':'icon.png'}
  if path in assets:
   p=self.server.web/assets[path];kind='text/html' if p.suffix=='.html' else 'text/javascript' if p.suffix=='.js' else 'text/css' if p.suffix=='.css' else 'image/png'
   return self.answer(200,p.read_bytes(),kind)
  if path.startswith('/prepared/') and self.server.job:
   try:index=int(path.rsplit('/',1)[1]);assert index>=0;p=Path(self.server.job['files'][index]['path'])
   except (ValueError,IndexError,AssertionError):return self.answer(404,{'error':'Fitxer no disponible'})
   self.downloadName='BASE_'+self.server.job['files'][index]['name']
   return self.answer(200,p.read_bytes(),'application/octet-stream')
  return self.answer(404,{'error':'No trobat'})
 def log_issue(self,message):
  inputs=getattr(self.server,'originals',[])
  record={'time':datetime.datetime.now().astimezone().isoformat(),'error':message,'inputs':[str(x) for x in inputs],'job':getattr(self.server,'job',None)}
  data=json.dumps(record,ensure_ascii=False,indent=2)
  paths=[];failures=[]
  for folder in dict.fromkeys(p.parent for p in inputs):
   try:
    path=self.unique_file(folder,'PPT-Merger-ERROR','.log')
    with path.open('x') as f:f.write(data)
    paths.append(str(path))
   except OSError as e:failures.append(str(folder)+': '+str(e))
  fallback=self.server.work/('error-'+uuid.uuid4().hex+'.log');fallback.write_text(data)
  return {'logs':paths,'logWriteErrors':failures,'fallbackLog':str(fallback)}
 def unique_file(self,folder,stem,extension):
  return folder/(stem+'-'+uuid.uuid4().hex[:12]+extension)
 @serialized
 def do_POST(self):
  if not self.allowed() or not secrets.compare_digest(self.headers.get('X-PPT-Token',''),self.server.token):return self.answer(403,{'error':'Accés no permès'})
  try:
   length=int(self.headers.get('Content-Length','0'))
   if not 0<length<=1024**3:raise ValueError('Mida no admesa (màxim 1 GiB per petició).')
   data=self.rfile.read(length);path=urlsplit(self.path).path
   if path=='/select':
    paths=self.native([self.server.binary,'--office-select'])
    if paths:
     self.server.job=None
     self.server.originals=[Path(x) for x in paths];self.server.uploads={}
     for i,p in enumerate(self.server.originals):self.server.uploads[str(i)]=p
    return self.answer(200,{'files':[{'id':k,'name':v.name} for k,v in self.server.uploads.items()]})
   if path=='/error':return self.answer(200,self.log_issue(json.loads(data).get('error','Error del complement')))
   if path=='/upload':raise ValueError('Selecciona els originals amb el selector natiu del Mac per conservar-ne la carpeta.')
   request=json.loads(data) if path not in ['/check-base','/verify'] else None
   if path=='/prepare':
    self.server.job=None
    ids=request['ids'];inputs=[self.server.uploads[x] for x in ids]
    self.server.originals=inputs
    if len(inputs)<2:raise ValueError('Calen dues presentacions.')
    if any(p.suffix.lower()=='.pptm' for p in inputs) and not request.get('experimentalPptm'):raise ValueError('PPTM pendent de validar amb Office.js. Activa la prova experimental expressament o usa còpies PPTX desades amb PowerPoint.')
    self.server.originals=inputs
    failures=[]
    for p in inputs:
     issues=preflight(p)
     if issues:failures.append({'file':p.name,'issues':issues})
    if failures:raise ValueError('Comprovació prèvia NO superada: '+json.dumps(failures,ensure_ascii=False))
    target=self.server.work/('prepared-'+uuid.uuid4().hex)
    command=[self.server.binary,'--office-prepare']+(['--fit-non169'] if request.get('fit') else [])+[str(target)]+[str(x) for x in inputs]
    result=self.native(command)
    self.server.job={**result,'id':uuid.uuid4().hex,'state':'prepared','engine':'Office.js KeepSourceFormatting','experimentalPptm':bool(request.get('experimentalPptm'))}
    base=self.unique_file(inputs[0].parent,'BASE_'+inputs[0].stem,inputs[0].suffix)
    with base.open('xb') as f:f.write(Path(result['files'][0]['path']).read_bytes())
    self.server.job['basePath']=str(base)
    self.server.job['preflightPassed']=True
    return self.answer(200,self.server.job)
   if not self.server.job:raise ValueError('No hi ha cap feina preparada.')
   if path=='/open-base':
    subprocess.run(['/usr/bin/open','-a','Microsoft PowerPoint',self.server.job['basePath']],check=True)
    return self.answer(200,{'ok':True})
   if path in ['/check-base','/verify']:
    output=self.server.work/('office-export-'+uuid.uuid4().hex+'.pptx');output.write_bytes(data)
    prepared=[x['path'] for x in self.server.job['files']]
    checks=self.native([self.server.binary,'--office-verify',str(output)]+(prepared[:1] if path=='/check-base' else prepared),allow_validation_failure=True)
    for i,check in enumerate(checks):check['file']=self.server.job['files'][i]['name']
    passed=all(not x['issues'] for x in checks)
    if path=='/check-base':
     self.server.job['baseChecked']=passed
     result={'passed':passed,'files':checks}
     if not passed:result.update(self.log_issue(result))
     return self.answer(200,result)
    saved=None
    if passed and getattr(self.server,'originals',None):
     base=Path(self.server.job['basePath'])
     saved=self.unique_file(self.server.originals[0].parent,'PPT-Merger-resultat',base.suffix)
     with saved.open('xb') as f:f.write(data)
    report={'savedOutput':str(saved) if saved else None,'engine':'Office.js KeepSourceFormatting','hostInsertionCompleted':True,'structuralChecksPassed':passed,'visualPlaybackValidated':False,'files':checks,'output':str(output),'warnings':['PPTM experimental; les macros de les entrades afegides no es fusionen.','La normalització prèvia de mides usa Open XML; no és una conversió de mida nativa de PowerPoint.']}
    output.with_suffix('.report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));self.server.job['report']=report;self.server.job['state']='verified'
    if not passed:report.update(self.log_issue(report))
    return self.answer(200,report)
   if path=='/begin':
    if self.server.job['state']!='prepared' or not self.server.job.get('baseChecked'):raise ValueError('Cal comprovar la base i començar una feina nova si ja hi ha una importació parcial.')
    self.server.job['state']='started';return self.answer(200,{'ok':True})
   if path=='/progress':
    self.server.job['progress']=request;return self.answer(200,{'ok':True})
   return self.answer(404,{'error':'No trobat'})
  except Exception as error:
   return self.answer(400,{'error':str(error),**self.log_issue(str(error))})
 def native(self,command,allow_validation_failure=False):
  result=subprocess.run(command,capture_output=True,text=True,timeout=900)
  if result.returncode and not (allow_validation_failure and result.returncode==2):raise ValueError(result.stderr.strip() or 'Error de preparació/comprovació')
  return json.loads(result.stdout)
def main():
 parser=argparse.ArgumentParser();parser.add_argument('--bin',required=True);parser.add_argument('--cert',required=True);parser.add_argument('--key',required=True);parser.add_argument('--work',required=True);args=parser.parse_args()
 server=BridgeServer(('127.0.0.1',PORT),Bridge);server.daemon_threads=True;server.token=secrets.token_urlsafe(32);server.job=None;server.uploads={};server.web=Path(__file__).parent/'web';server.work=Path(args.work);server.work.mkdir(parents=True,exist_ok=True);server.binary=str(Path(args.bin).resolve())
 context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.minimum_version=ssl.TLSVersion.TLSv1_2;context.load_cert_chain(args.cert,args.key);server.socket=context.wrap_socket(server.socket,server_side=True,do_handshake_on_connect=False)
 print(f'PPT Merger Office.js: https://localhost:{PORT} · Ctrl+C per aturar',flush=True)
 server.serve_forever()
if __name__=='__main__':main()

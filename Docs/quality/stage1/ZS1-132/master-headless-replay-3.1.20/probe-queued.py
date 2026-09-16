from pathlib import Path
import argparse, hashlib, json, os, sys, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0,str(Path.cwd()/'tools'))
from tui_session_new_smoke import Jsonl

parser=argparse.ArgumentParser()
parser.add_argument('--binary',type=Path,required=True)
parser.add_argument('--evidence',type=Path,required=True)
args=parser.parse_args();binary=args.binary.resolve();evidence=args.evidence.resolve()
root=evidence.with_suffix('.fixture');config=root/'fixture'
for path in [root/'initial',root/'other',root/'sessions',config]:path.mkdir(parents=True)
entered=threading.Event();release=threading.Event();entered_first=threading.Event();release_first=threading.Event();requests=[];wires=[];errors=[]
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  try:
   body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
   if len(requests)==1:
    entered_first.set();assert release_first.wait(10),'first provider gate timed out'
   elif len(requests)==2:
    entered.set();assert release.wait(10),'queued provider gate timed out'
   events=[{'type':'response.output_text.delta','delta':'ONE_REPLY'},
           {'type':'response.completed','response':{'id':'gate','status':'completed'}}]
   raw=''.join('data: '+json.dumps(e)+'\n\n' for e in events).encode()
   self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Content-Length',str(len(raw)));self.end_headers();self.wfile.write(raw)
  except BaseException as e:errors.append(repr(e))
server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
(config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_')) and k.lower() not in ('http_proxy','https_proxy','all_proxy','no_proxy')}
env.update(ZENPI_HOME=str(config),TERM='xterm-256color')
result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':{}}
try:
 w=Jsonl(binary,root,env);wires.append(w)
 w.send('initial-status',type='status');initial=w.response('initial-status');assert initial['success'],initial
 w.send('open-b',type='project',project={'action':'open','cwd':str(root/'other')});opened=w.response('open-b');assert opened['success'],opened
 a=initial['project']['project_id'];b=opened['project']['project_id']
 def select(id,project):
  w.send(id,type='project',project={'action':'select','id':project});r=w.response(id);assert r['success'],r
 select('select-a',a)
 w.send('hold-a',type='prompt',text='FIRST_GATE_A')
 assert entered_first.wait(5),'first provider never reached'
 select('select-b-before-new',b)
 w.send('new-b',type='command',text='/new')
 select('select-a-before-queue',a)
 w.send('pending-a',type='prompt',text='ORIGINAL_PROJECT_A_ONLY')
 select('select-b-before-release',b)
 release_first.set()
 created=w.response('new-b');assert created['success'],created
 assert entered.wait(5),'queued A provider never reached'
 result['checks']['new_b_committed_before_queued_a_terminal']=True
 w.send('pending-a',type='prompt',text='ORIGINAL_PROJECT_A_ONLY')
 w.send('barrier',type='status');barrier=w.response('barrier');assert barrier['success'],barrier
 duplicate=[r for r in w.rows if r.get('id')=='pending-a' and r.get('code')=='duplicate_request_in_flight']
 result['checks']['inflight_retry_stays_reserved_after_other_project_new']=bool(duplicate)
 release.set()
 w.close()
 terminal=[r for r in w.rows if r.get('type')=='response' and r.get('id')=='pending-a' and r.get('success')]
 result['checks']['one_provider_call_one_success_terminal']=len(requests)==2 and len(terminal)==1 and terminal[0].get('project',{}).get('project_id')==a
 result.update(initial_project=initial.get('project'),new_project=created.get('project'),request_count=len(requests),terminals=terminal)
 assert not errors,errors
 assert all(result['checks'].values()),result
 result['status']='passed'
except BaseException as error:
 result.update(status='failed',error=repr(error));raise
finally:
 release_first.set();release.set()
 for w in wires:
  if w.process.poll() is None:w.kill()
 server.shutdown();server.server_close()
 evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
 evidence.with_suffix('.jsonl.json').write_text(json.dumps([{'pid':w.process.pid,'returncode':w.process.returncode,'rows':w.rows,'stderr':w.errors} for w in wires],ensure_ascii=False,indent=2)+'\n')
 evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n')

#!/usr/bin/env python3
import argparse,hashlib,json,os,sys,threading,time,subprocess,traceback
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
sys.path.insert(0,str(Path(__file__).resolve().parent/'pty-support'))
from tui_project_workspace_smoke import Terminal,screen_text,drain
from cli_transport import CLI

def sha(b):return hashlib.sha256(b).hexdigest()
def save(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def main():
 p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--fixture-binary',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args();binary=a.binary.resolve();seed=a.fixture_binary.resolve();out=a.evidence.resolve();out.mkdir(parents=True,exist_ok=False);root=out/'fixture';config=root/'config'
 for x in [root/'initial',root/'sessions',config]:x.mkdir(parents=True)
 journal=root/'sessions/initial.jsonl';effect=root/'initial/effect-count.txt';checkpoint=config/'project-tabs.json';requests=[];replies=[];events=[];checks={};t=None;cli=None;terminals=[];result={'passed':False,'binary_sha256':sha(binary.read_bytes()),'fixture_binary_sha256':sha(seed.read_bytes()),'checks':checks}
 def event(kind,**data):events.append(dict(kind=kind,monotonic_ns=time.monotonic_ns(),**data));save(out/'timeline.json',events)
 class Handler(BaseHTTPRequestHandler):
  def log_message(self,*a):pass
  def do_POST(self):
   raw=self.rfile.read(int(self.headers['Content-Length']));body=json.loads(raw);idx=len(requests)+1;requests.append(dict(index=idx,path=self.path,body=body,monotonic_ns=time.monotonic_ns()));save(out/'http-requests.json',requests)
   latest=next(m for m in reversed(body['messages']) if m['role']=='user')['content'];has_result=any(m.get('role')=='tool' and m.get('tool_call_id')=='counter-call' for m in body['messages'])
   if latest=='BRANCH_C_EFFECT' and not has_result:
    msg={'role':'assistant','content':None,'tool_calls':[{'id':'counter-call','type':'function','function':{'name':'run_command','arguments':json.dumps({'command':"printf 'EXECUTED\\n' >> effect-count.txt",'timeout_ms':10000})}}]};finish='tool_calls'
   else:msg={'role':'assistant','content':'COMPLETED_'+latest};finish='stop'
   reply={'model':body['model'],'choices':[{'index':0,'message':msg,'finish_reason':finish}],'usage':{'prompt_tokens':15000,'completion_tokens':20,'total_tokens':15020}};replies.append(reply);save(out/'http-responses.json',replies);data=json.dumps(reply).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data);self.wfile.flush()
 server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
 (config/'config.toml').write_text(f'backend="openai"\nprovider="openai"\nmodel="select-cancel-fixture"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="chat"\nrequires_openai_auth=false\nmax_retries=0\n[[model_overrides]]\nprovider="openai"\nid="select-cancel-fixture"\nversion="select-cancel-v1"\ncontext_window=262144\nmax_output_tokens=4096\ntools=true\nstreaming=false\n')
 env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(config),TERM='xterm-256color')
 def rows():return [json.loads(s) for s in journal.read_text().splitlines()]
 def leaf():
  for r in reversed(rows()):
   if 'tree_entry' in r:return r['tree_entry']['id']
   e=r.get('event',{})
   if e.get('type')=='session_tree' and e['action']['kind'] in ('select','migrate'):return e['action']['leaf']
 def active():
  c=json.loads(checkpoint.read_text());return next(x for x in c['project_state'] if x['name']==c['projects'][c['active']])
 def messages():return active()['messages']
 def text():return json.dumps(messages())
 def idle():t.wait(lambda:b'Ready' in screen_text(bytes(t.output)).splitlines()[1])
 def count():assert effect.read_bytes()==b'EXECUTED\n'
 def snap(name):
  d=out/name;d.mkdir();(d/'journal.jsonl').write_bytes(journal.read_bytes());(d/'project-tabs.json').write_bytes(checkpoint.read_bytes());(d/'terminal.ansi').write_bytes(t.output);(d/'screen.txt').write_bytes(screen_text(bytes(t.output)))
  if effect.exists():(d/'effect-count.txt').write_bytes(effect.read_bytes())
  event('snapshot',name=name,leaf=leaf(),journal_sha256=sha(journal.read_bytes()),http_requests=len(requests))
 try:
  with open(out/'fixture-seed.log','wb') as log:subprocess.run([str(seed),'--exact','tui_navigation_race_fixture','--ignored','--nocapture'],env=dict(env,ZS1_TUI_TREE_FIXTURE=str(journal),ZS1_TUI_TREE_CWD=str(root/'initial')),stdout=log,stderr=subprocess.STDOUT,timeout=60,check=True)
  (out/'seeded-session.jsonl').write_bytes(journal.read_bytes());assert sum('turn' in r for r in rows())==1500;event('synthetic_common_history_seeded',entries=1500)
  t=Terminal(binary,root,env);terminals.append(t);event('tui_started',pid=t.pid);t.wait(lambda:checkpoint.exists());idle()
  for prompt in ['COMMON_A_REAL','BRANCH_B_ONLY']:
   t.command(prompt);t.expect(('COMPLETED_'+prompt).encode());idle()
   if prompt=='COMMON_A_REAL':node_a=leaf()
  node_b=leaf();t.command('/tree select '+node_a);t.wait(lambda:leaf()==node_a and 'BRANCH_B_ONLY' not in text());idle()
  t.command('BRANCH_C_EFFECT');t.wait(lambda:b'Approvalrequired' in b''.join(screen_text(bytes(t.output)).splitlines()[1].split()));assert not effect.exists();snap('approval-before-effect');event('approval_visible_before_effect')
  t.write(b'y\r');t.expect(b'COMPLETED_BRANCH_C_EFFECT');idle();count();node_c=leaf();assert len(requests)==4;assert 'BRANCH_C_EFFECT' in text() and 'BRANCH_B_ONLY' not in text();snap('before-select')
  node_b=json.loads((out/'fixture-seed.log').read_text().splitlines()[-1])['target'];before=journal.read_bytes();prior_messages=messages();prior_layout=active()['layout'];prior_cwd=active()['metadata']['cwd'];prior_replies=len(requests)
  t.close();event('tui_closed_before_cli')
  cli=CLI(binary,root,out,'cancel-cli',env);cli.request('ready','status');sid=rows()[0]['session_id'];page=cli.tree('before-cancel-leaf',sid,action='list',cursor=0,limit=1);assert page['data']['active_leaf']==node_c
  before=journal.read_bytes();(out/'pre-cli-cancel.jsonl').write_bytes(before);before_rows=rows()
  select=dict(schema_version=2,type='tree',id='cancel-this-select',session_id=sid,tree=dict(action='select',leaf=node_b))
  cancel=dict(schema_version=2,type='cancel',id='cancel-select-now',target_id='cancel-this-select')
  payload=('\n'.join(json.dumps(v) for v in [select,cancel])+'\n').encode();cli.stdin.write(payload);cli.stdin.flush();event('compiled_select_and_target_cancel_sent',from_leaf=node_c,target_leaf=node_b);cli.p.stdin.write(payload);cli.p.stdin.flush()
  cr=cli.response('cancel-select-now');sr=cli.wait(lambda r:r.get('type')=='response' and r.get('id')=='cancel-this-select');save(out/'cancel-response.json',cr);save(out/'select-response.json',sr);assert sr.get('success') is False,sr
  assert 'cancel' in json.dumps(sr).lower(),sr
  (out/'post-cli-cancel.jsonl').write_bytes(journal.read_bytes());added=rows()[len(before_rows):];assert journal.read_bytes().startswith(before)
  assert not any('turn' in r or r.get('event',{}).get('type')=='session_tree' for r in added),added
  page=cli.tree('after-cancel-leaf',sid,action='list',cursor=0,limit=1);assert page['data']['active_leaf']==node_c;save(out/'after-cancel-leaf.json',page);assert leaf()==node_c and len(requests)==prior_replies;count()
  event('compiled_select_cancelled_before_commit',select_response=sr,cancel_response=cr)
  cli.request('stop-after-cancel','shutdown');code=cli.p.wait(timeout=10);assert code==0;cli.reader.join(timeout=5);cli.receipt.update(returncode=code,normal_shutdown=True);save(cli.base/'process.json',cli.receipt)
  for f in [cli.stdout,cli.stderr,cli.stdin,cli.p.stdin,cli.p.stdout]:f.close()
  t=Terminal(binary,root,env);terminals.append(t);event('tui_reopened_after_cancel',pid=t.pid);t.wait(lambda:checkpoint.exists() and 'BRANCH_C_EFFECT' in text());idle();assert leaf()==node_c and len(requests)==prior_replies;count();snap('after-cancel')
  after_messages=messages();assert 'BRANCH_C_EFFECT' in text() and 'COMMON_A_REAL' in text() and 'BRANCH_B_ONLY' not in text()
  common=[m for m in prior_messages if m in after_messages]
  assert all(any(n==m for n in after_messages) for m in prior_messages if m.get('text') in ['COMMON_A_REAL','COMPLETED_COMMON_A_REAL','BRANCH_C_EFFECT','COMPLETED_BRANCH_C_EFFECT'])
  assert active()['layout']==prior_layout and active()['metadata']['cwd']==prior_cwd
  checks.update(compiled_cli_selection_cancelled_before_commit=True,original_leaf_and_tree_history_unchanged=True,original_branch_transcript_retained_in_reopened_tui=True,no_navigation_http_or_tool_effect=True)
  t.clear_input();t.command('AFTER_CANCEL_CONTINUE');t.expect(b'COMPLETED_AFTER_CANCEL_CONTINUE');idle();count();assert len(requests)==prior_replies+1
  body=requests[-1]['body'];wire=json.dumps(body);assert 'COMMON_A_REAL' in wire and 'BRANCH_C_EFFECT' in wire and 'BRANCH_B_ONLY' not in wire;assert body['messages'][-1]['content']=='AFTER_CANCEL_CONTINUE';assert sum(m.get('role')=='tool' for m in body['messages'])==1
  assert sum(r.get('event',{}).get('type')=='tool_execution_finished' for r in rows())==1;assert sum(r.get('turn',{}).get('role')=='tool' for r in rows())==1;snap('after-real-continuation');checks.update(subsequent_actual_http_uses_original_branch_and_tool_result=True,actual_tool_executes_once=True)
  t.close();checks['terminal_restored']=True;result.update(passed=True,tui_processes=2,cli_processes=1,http_requests=len(requests),actual_tool_executions=1,original_leaf=node_c,cancelled_target=node_b,retained_transcript_messages=len(common),prior_transcript_messages=len(prior_messages),cancellation_boundary='compiled JSONL tree select plus public target cancel; failed cancelled selection response; TUI before/after; internal projection phase not instrumented')
 except Exception:result['error']=traceback.format_exc();raise
 finally:
  for n,term in enumerate(terminals):(out/f'terminal-{n}.ansi').write_bytes(term.output);term.cleanup()
  if cli and cli.p.poll() is None:cli.kill({'failure_cleanup':True})
  save(out/'result.json',result);server.shutdown();server.server_close()
 print(json.dumps(result,ensure_ascii=False,indent=2))
if __name__=='__main__':main()

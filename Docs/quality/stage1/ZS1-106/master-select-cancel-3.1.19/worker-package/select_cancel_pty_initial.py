#!/usr/bin/env python3
import argparse,hashlib,json,os,sys,threading,time,subprocess,traceback
from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
sys.path.insert(0,str(Path(__file__).resolve().parent/'pty-support'))
from tui_project_workspace_smoke import Terminal,screen_text,drain

def sha(b):return hashlib.sha256(b).hexdigest()
def save(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def main():
 p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--fixture-binary',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args();binary=a.binary.resolve();seed=a.fixture_binary.resolve();out=a.evidence.resolve();out.mkdir(parents=True,exist_ok=False);root=out/'fixture';config=root/'config'
 for x in [root/'initial',root/'sessions',config]:x.mkdir(parents=True)
 journal=root/'sessions/initial.jsonl';effect=root/'initial/effect-count.txt';checkpoint=config/'project-tabs.json';requests=[];replies=[];events=[];checks={};t=None;result={'passed':False,'binary_sha256':sha(binary.read_bytes()),'fixture_binary_sha256':sha(seed.read_bytes()),'checks':checks}
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
  t=Terminal(binary,root,env);event('tui_started',pid=t.pid);t.wait(lambda:checkpoint.exists());idle()
  for prompt in ['COMMON_A_REAL','BRANCH_B_ONLY']:
   t.command(prompt);t.expect(('COMPLETED_'+prompt).encode());idle()
   if prompt=='COMMON_A_REAL':node_a=leaf()
  node_b=leaf();t.command('/tree select '+node_a);t.wait(lambda:leaf()==node_a and 'BRANCH_B_ONLY' not in text());idle()
  t.command('BRANCH_C_EFFECT');t.wait(lambda:b'Approvalrequired' in b''.join(screen_text(bytes(t.output)).splitlines()[1].split()));assert not effect.exists();snap('approval-before-effect');event('approval_visible_before_effect')
  t.write(b'y\r');t.expect(b'COMPLETED_BRANCH_C_EFFECT');idle();count();node_c=leaf();assert len(requests)==4;assert 'BRANCH_C_EFFECT' in text() and 'BRANCH_B_ONLY' not in text();snap('before-select')
  before=journal.read_bytes();prior_messages=messages();prior_layout=active()['layout'];prior_cwd=active()['metadata']['cwd'];prior_replies=len(requests)
  command='/tree select '+node_b;t.write(command.encode());t.wait(lambda:t.draft()==command)
  deadline=time.monotonic()+.15
  while time.monotonic()<deadline:drain(t.fd,t.output,.01)
  event('select_then_interrupt_written',from_leaf=node_c,target_leaf=node_b,bytes='deliberate Enter followed immediately by Ctrl-C');t.write(b'\r\x03')
  def interrupted():
   status=screen_text(bytes(t.output)).splitlines()[1]
   return b'Interrupted' in status or (b'Request failed' in status and 'cancel' in text().lower())
  t.wait(interrupted);snap('after-cancel');event('actual_cancelled_terminal_status',status=screen_text(bytes(t.output)).splitlines()[1].decode())
  assert leaf()==node_c,'select won before cancellation; no cancelled-selection evidence'
  assert journal.read_bytes()==before,'cancelled navigation altered journal'
  assert len(requests)==prior_replies;count();after_messages=messages()
  assert 'BRANCH_C_EFFECT' in text() and 'COMMON_A_REAL' in text() and 'BRANCH_B_ONLY' not in text()
  # Existing bounded transcript can drop its oldest prefix when adding control notices.
  common=[m for m in prior_messages if m in after_messages]
  assert len(common)>=len(prior_messages)-3,(len(common),len(prior_messages))
  assert active()['layout']==prior_layout and active()['metadata']['cwd']==prior_cwd
  checks.update(compiled_tui_selection_cancelled_before_commit=True,original_leaf_and_journal_unchanged=True,original_branch_transcript_retained=True,no_navigation_http_or_tool_effect=True)
  t.clear_input();t.command('AFTER_CANCEL_CONTINUE');t.expect(b'COMPLETED_AFTER_CANCEL_CONTINUE');idle();count();assert len(requests)==prior_replies+1
  body=requests[-1]['body'];wire=json.dumps(body);assert 'COMMON_A_REAL' in wire and 'BRANCH_C_EFFECT' in wire and 'BRANCH_B_ONLY' not in wire;assert body['messages'][-1]['content']=='AFTER_CANCEL_CONTINUE';assert sum(m.get('role')=='tool' for m in body['messages'])==1
  assert sum(r.get('event',{}).get('type')=='tool_execution_finished' for r in rows())==1;assert sum(r.get('turn',{}).get('role')=='tool' for r in rows())==1;snap('after-real-continuation');checks.update(subsequent_actual_http_uses_original_branch_and_tool_result=True,actual_tool_executes_once=True)
  t.close();checks['terminal_restored']=True;result.update(passed=True,tui_processes=1,http_requests=len(requests),actual_tool_executions=1,original_leaf=node_c,cancelled_target=node_b,retained_transcript_messages=len(common),prior_transcript_messages=len(prior_messages),cancellation_boundary='public Enter then Ctrl-C; observed Interrupted terminal, internal projection phase not instrumented')
 except Exception:result['error']=traceback.format_exc();raise
 finally:
  if t:(out/'terminal.ansi').write_bytes(t.output);t.cleanup()
  save(out/'result.json',result);server.shutdown();server.server_close()
 print(json.dumps(result,ensure_ascii=False,indent=2))
if __name__=='__main__':main()

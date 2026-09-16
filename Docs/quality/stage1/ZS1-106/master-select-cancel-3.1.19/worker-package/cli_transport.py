#!/usr/bin/env python3
"""Real CLI + HTTP + approved append side effect, kill-after-receipt recovery.
Never edits journals or injects turns. --evidence must be a new directory.
"""
import argparse, hashlib, json, os, queue, shutil, signal, subprocess, threading, time, traceback
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

def digest(b): return hashlib.sha256(b).hexdigest()
def save(path, value): path.write_text(json.dumps(value, ensure_ascii=False, indent=2)+'\n')
def now(): return {'unix_ns':time.time_ns(),'monotonic_ns':time.monotonic_ns()}

class CLI:
    def __init__(self, binary, root, evidence, name, env):
        self.name=name; self.base=evidence/name; self.base.mkdir(); self.q=queue.Queue(); self.pending=[]; self.rows=[]
        self.stdout=open(self.base/'stdout.jsonl','wb'); self.stderr=open(self.base/'stderr.log','wb'); self.stdin=open(self.base/'stdin.jsonl','wb')
        argv=[str(binary),'--mode','headless','--session',str(root/'sessions/initial.jsonl')]
        self.p=subprocess.Popen(argv,cwd=root/'initial',env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.stderr)
        self.receipt={'argv':argv,'cwd':str(root/'initial'),'pid':self.p.pid,'start':now()}
        save(self.base/'process.json',self.receipt)
        def read():
            try:
                for line in self.p.stdout:
                    self.stdout.write(line); self.stdout.flush(); row=json.loads(line); self.rows.append(row); self.q.put(row)
            finally: self.q.put(None)
        self.reader=threading.Thread(target=read,daemon=True); self.reader.start()
    def send(self, data):
        raw=(json.dumps(data)+'\n').encode();self.stdin.write(raw);self.stdin.flush();self.p.stdin.write(raw);self.p.stdin.flush()
    def wait(self, pred, timeout=20):
        for i,row in enumerate(self.pending):
            if pred(row): return self.pending.pop(i)
        deadline=time.monotonic()+timeout
        while True:
            row=self.q.get(timeout=max(.01,deadline-time.monotonic()))
            assert row is not None, 'CLI EOF: '+str(self.base/'stderr.log')
            if pred(row): return row
            self.pending.append(row)
    def response(self,rid):
        r=self.wait(lambda r:r.get('type')=='response' and r.get('id')==rid)
        assert r.get('success') is True, r
        return r
    def request(self,rid,kind,**fields):
        self.send(dict(schema_version=2,id=rid,type=kind,**fields));return self.response(rid)
    def tree(self,rid,sid,**action): return self.request(rid,'tree',session_id=sid,tree=action)
    def kill(self,boundary):
        assert self.p.poll() is None
        self.receipt.update(kill_boundary=boundary,kill_sent=now(),signal='SIGKILL')
        self.p.send_signal(signal.SIGKILL);self.receipt['returncode']=self.p.wait(timeout=10);self.receipt['reaped']=now()
        self.reader.join(timeout=5);assert not self.reader.is_alive();assert self.receipt['returncode']==-signal.SIGKILL
        save(self.base/'process.json',self.receipt);self.stdout.close();self.stderr.close();self.stdin.close();self.p.stdin.close();self.p.stdout.close()

def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('--binary',type=Path,required=True);ap.add_argument('--evidence',type=Path,required=True);ap.add_argument('--case',choices=['selected-tool','selected-sibling'],required=True);args=ap.parse_args()
    binary=args.binary.resolve();out=args.evidence.resolve();out.mkdir(parents=True,exist_ok=False);root=out/'fixture'
    for folder in ['work','user','sessions']:(root/folder).mkdir(parents=True)
    requests=[];responses=[];server_errors=[];children=[];checks={};timeline=[]
    report={'case':args.case,'binary_sha256':digest(binary.read_bytes()),'checks':checks,'passed':False,'boundary':'SIGKILL only after successful durable CLI terminal response, not during fsync'}
    def event(kind,**data): timeline.append(dict(kind=kind,**now(),**data));save(out/'timeline.json',timeline)
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*a):pass
        def do_POST(self):
            try:
                raw=self.rfile.read(int(self.headers['Content-Length']));body=json.loads(raw);index=len(requests)+1
                request={'index':index,'path':self.path,'body':body,'raw_sha256':digest(raw),**now()};requests.append(request);save(out/'http-requests.json',requests)
                latest=next(m for m in reversed(body['messages']) if m.get('role')=='user')['content']
                has_result=any(m.get('role')=='tool' and m.get('tool_call_id')=='counter-call' for m in body['messages'])
                if latest=='BRANCH_B_EFFECT' and not has_result:
                    message={'role':'assistant','content':None,'tool_calls':[{'id':'counter-call','type':'function','function':{'name':'run_command','arguments':json.dumps({'command':"printf 'EXECUTED\\n' >> effect-count.txt",'timeout_ms':10000})}}]};finish='tool_calls'
                else:message={'role':'assistant','content':'COMPLETED_'+latest};finish='stop'
                reply={'model':body['model'],'choices':[{'index':0,'message':message,'finish_reason':finish}],'usage':{'prompt_tokens':100,'completion_tokens':20,'total_tokens':120}}
                encoded=json.dumps(reply).encode();responses.append({'index':index,'body':reply,'raw_sha256':digest(encoded),**now()});save(out/'http-responses.json',responses)
                self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(encoded)));self.end_headers();self.wfile.write(encoded);self.wfile.flush()
            except Exception:server_errors.append(traceback.format_exc());save(out/'http-errors.json',server_errors);raise
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env['ZENPI_HOME']=str(root/'user')
    (root/'user/config.toml').write_text(f'backend="openai"\nprovider="openai"\nmodel="tree-persistence-fixture"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="chat"\nrequires_openai_auth=false\nmax_retries=0\n[[model_overrides]]\nprovider="openai"\nid="tree-persistence-fixture"\nversion="cli-persistence-v1"\ncontext_window=32768\nmax_output_tokens=4096\ntools=true\nstreaming=false\n')
    journal=root/'sessions/initial.jsonl';effect=root/'work/effect-count.txt'
    def records():return [json.loads(line) for line in journal.read_text().splitlines()]
    def snap(name):
        target=out/'snapshots'/name;target.mkdir(parents=True);items=[]
        for p in sorted(root.rglob('*')):
            if p.is_file() and not p.is_symlink():
                b=p.read_bytes();q=target/p.relative_to(root);q.parent.mkdir(parents=True,exist_ok=True);q.write_bytes(b);items.append({'path':str(p.relative_to(root)),'bytes':len(b),'sha256':digest(b)})
        save(target/'hashes.json',items);event('snapshot',name=name,journal_sha256=digest(journal.read_bytes()),effect_sha256=digest(effect.read_bytes()) if effect.exists() else None)
    def count_check():assert effect.read_bytes()==b'EXECUTED\n',effect.read_bytes()
    def page(cli,rid,sid):return cli.tree(rid,sid,action='list',cursor=0,limit=128)['data']
    def validate_history(selected):
        rs=records();entries={r['tree_entry']['id']:r for r in rs if 'tree_entry' in r};path=[];cursor=selected
        while cursor:
            row=entries[cursor];path.append(row['turn']);cursor=row['tree_entry']['parent_id']
        path.reverse();text=json.dumps(path);assert 'COMMON_A' in text
        assert ('BRANCH_B_EFFECT' in text)==(args.case=='selected-tool');assert ('BRANCH_C_ONLY' in text)==(args.case=='selected-sibling')
        assert sum(t['role']=='tool' for t in path)==(1 if args.case=='selected-tool' else 0)
        assert sum(r.get('turn',{}).get('role')=='tool' for r in rs)==1
        assert any(r.get('turn',{}).get('content')=='BRANCH_B_EFFECT' for r in rs)
        assert any(r.get('turn',{}).get('content')=='BRANCH_C_ONLY' for r in rs)
        return [t['id'] for t in path]
    try:
        cli=CLI(binary,root,out,'create',env);children.append(cli);cli.request('initial-status','status');sid=records()[0]['session_id']
        cli.tree('enable',sid,action='enable');cli.request('create-a','prompt',text='COMMON_A');a=page(cli,'leaf-a',sid)['active_leaf']
        original=dict(schema_version=2,id='create-b',type='prompt',text='BRANCH_B_EFFECT');cli.send(original)
        approval=cli.wait(lambda r:r.get('event',{}).get('type')=='approval_request');assert not effect.exists();event('approval_boundary',approval=approval,effect_exists=False)
        cli.request('approve-b','approve',approval_id=approval['event']['approval']['request_id'],decision='allow');b_response=cli.response('create-b');count_check();b=page(cli,'leaf-b',sid)['active_leaf']
        finished=[r for r in records() if r.get('event',{}).get('type')=='tool_execution_finished'];assert len(finished)==1 and finished[0]['event']['outcome']=='succeeded'
        snap('tool-complete');checks['real_tool_requires_public_approval_and_appends_exactly_once']=True
        cli.tree('back-a',sid,action='select',leaf=a);cli.request('create-c','prompt',text='BRANCH_C_ONLY');c=page(cli,'leaf-c',sid)['active_leaf']
        target=b if args.case=='selected-tool' else c;annotation={'name':'恢复分支','summary':'durable display summary '+args.case}
        cli.tree('annotate-target',sid,action='annotate',entry_id=target,annotation=annotation)
        cli.tree('force-other',sid,action='select',leaf=c if target==b else b)
        terminal=cli.tree('select-target',sid,action='select',leaf=target);assert terminal['data']['active_leaf']==target
        saved=next(r for r in reversed(records()) if r.get('event',{}).get('type')=='session_tree');assert saved['event']['action']['leaf']==target
        ancestry=validate_history(target);count_check();assert len(requests)==4;event('durable_selection_response',response=terminal,journal_sequence=saved['seq'],leaf=target,ancestry=ancestry)
        snap('before-kill-create');cli.kill({'response_id':'select-target','journal_sequence':saved['seq'],'leaf':target});snap('after-kill-create')
        checks['kill_after_committed_selection']=True
        for n in [1,2]:
            cli=CLI(binary,root,out,f'recover-{n}',env);children.append(cli);before=len(requests);view=page(cli,f'recovery-view-{n}',sid)
            assert view['active_leaf']==target and next(v for v in view['nodes'] if v['entry']['id']==target)['annotation']==annotation
            assert validate_history(target)==ancestry;count_check();assert len(requests)==before
            cli.send(original);replayed=cli.response('create-b');assert replayed==b_response,(replayed,b_response)
            assert page(cli,f'after-replay-{n}',sid)['active_leaf']==target;assert len(requests)==before;count_check()
            continuation=f'RECOVERY_CONTINUE_{n}';cli.request(f'continue-{n}','prompt',text=continuation);assert len(requests)==before+1
            body=requests[-1]['body'];wire=json.dumps(body);assert body['messages'][-1]['content']==continuation
            assert 'COMMON_A' in wire;assert ('BRANCH_B_EFFECT' in wire)==(args.case=='selected-tool');assert ('BRANCH_C_ONLY' in wire)==(args.case=='selected-sibling')
            assert sum(m.get('role')=='tool' for m in body['messages'])==(1 if args.case=='selected-tool' else 0)
            count_check();assert not any(r.get('event',{}).get('type')=='approval_request' for r in cli.rows)
            cli.tree(f'restore-target-{n}',sid,action='select',leaf=target);assert validate_history(target)==ancestry
            snap(f'before-kill-recover-{n}');cli.kill({'response_id':f'restore-target-{n}','leaf':target});snap(f'after-kill-recover-{n}')
            checks[f'independent_recovery_{n}_leaf_annotation_ancestry_http_and_replay_stable']=True
        assert len({p.p.pid for p in children})==3;assert len(requests)==6;assert not server_errors
        report.update(passed=True,http_requests=len(requests),actual_tool_executions=1,cli_processes=3,sigkill_reaped=3,selected_leaf=target,selected_ancestry=ancestry,session_id=sid)
    except Exception:
        report['error']=traceback.format_exc();raise
    finally:
        for cli in children:
            if cli.p.poll() is None:cli.kill({'cleanup_after_failure':True})
        server.shutdown();server.server_close();thread.join(timeout=5);save(out/'result.json',report)
    print(json.dumps(report,ensure_ascii=False,indent=2))
if __name__=='__main__':main()

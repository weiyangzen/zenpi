#!/usr/bin/env python3
"""Run actual Stage1 UX entry points against one immutable production binary.

This harness reports behavioral evidence only. It never marks Blueprint items
accepted, substitutes old results, or changes the selected binary/source tree.
"""
from __future__ import annotations
import argparse
import fcntl
import hashlib
import json
import os
import platform
import queue
import re
import struct
import subprocess
import sys
import signal
import termios
import threading
import time
from pathlib import Path
from tempfile import TemporaryDirectory
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from tui_project_workspace_smoke import Terminal, screen_text
from tui_user_shell_smoke import drain

ROOT = Path(__file__).resolve().parents[1]


def cleanup_terminal(terminal):
    if terminal.pid is None:
        return
    try:
        os.kill(terminal.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        try:
            if os.waitpid(terminal.pid, os.WNOHANG)[0]:
                terminal.pid = None
                break
        except ChildProcessError:
            terminal.pid = None
            break
        drain(terminal.fd, terminal.output, .03)
    os.close(terminal.fd)


def run_shared_projects(binary: Path, evidence: Path):
    requests=[];seen=set();gate=threading.Event();release=threading.Event();processes=[];terminals=[]
    class Handler(BaseHTTPRequestHandler):
     def log_message(self,*a):pass
     def do_POST(self):
      body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
      latest=next((v for v in reversed(body['input']) if v.get('role')=='user'),{})
      marker=next((m for m in ['SLOW_A','QUEUED_B'] if m in json.dumps(latest)),None)
      self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
      def emit(kind,**values):self.wfile.write(('data: '+json.dumps(dict(type=kind,**values))+'\n\n').encode());self.wfile.flush()
      try:
       emit('response.created',response={'id':str(len(requests)),'model':body['model']})
       if marker and marker not in seen:
        seen.add(marker)
        if marker=='SLOW_A':
         gate.set()
         while not release.wait(.03):emit('response.output_text.delta',delta='')
        emit('response.output_item.done',item={'type':'function_call','call_id':marker,'name':'write_file','arguments':json.dumps({'path':'marker.txt','content':marker})})
       else:emit('response.output_text.delta',delta='DONE_'+str(marker))
       emit('response.completed',response={'id':str(len(requests)),'status':'completed','model':body['model']})
      except (BrokenPipeError,ConnectionResetError):pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
    class Wire:
     def __init__(self,root,env):
      self.p=subprocess.Popen([str(binary),'--mode','headless','--session',str(root/'sessions/initial.jsonl')],cwd=root/'initial',env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1);processes.append(self);self.q=queue.Queue();self.records=[];self.pending=[]
      def reader():
       for line in self.p.stdout:self.q.put(json.loads(line))
       self.q.put(None)
      threading.Thread(target=reader,daemon=True).start()
     def send(self,**values):self.p.stdin.write(json.dumps(dict(schema_version=2,**values))+'\n');self.p.stdin.flush()
     def wait(self,pred):
      for index,value in enumerate(self.pending):
       if pred(value):return self.pending.pop(index)
      until=time.monotonic()+15
      while time.monotonic()<until:
       v=self.q.get(timeout=max(.01,until-time.monotonic()))
       assert v is not None,'unexpected EOF '+self.p.stderr.read()
       self.records.append(v)
       if pred(v):return v
       self.pending.append(v)
      raise AssertionError('timeout')
     def response(self,id):return self.wait(lambda v:v.get('type')=='response' and v.get('id')==id)
     def project(self,rid,**control):self.send(type='project',id=rid,project=control);return self.response(rid)
     def close(self):
      self.send(type='shutdown',id='stop-'+str(len(processes)));assert self.response('stop-'+str(len(processes)))['success'];self.p.stdin.close();assert self.p.wait(timeout=8)==0,self.p.stderr.read()
    result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':{}}
    try:
     with TemporaryDirectory(prefix='project126-production-',dir=Path('.ops').resolve()) as raw:
      root=Path(raw);config=root/'fixture';other=root/'other';bad=root/'invalid'
      for d in [root/'initial',root/'sessions',config,other/'.zenpi',bad/'.zenpi']:d.mkdir(parents=True)
      (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
      (other/'.zenpi/config.toml').write_text('model="gpt-4o-mini"\n');(bad/'.zenpi/config.toml').write_text('model=[ broken')
      (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'project126-fixture'}));(config/'auth.json').chmod(0o600)
      env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(config),TERM='xterm-256color',NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
      w=Wire(root,env);initial=w.project('initial',action='list');a=initial['project']['project_id']
      w.send(type='prompt',id='slow-a',text='SLOW_A');assert gate.wait(5)
      opened=w.project('open-b',action='open',cwd=str(other));assert opened['success'];b=opened['project']['project_id'];assert b!=a;result['checks']['open_prepares_real_second_owner_while_provider_waits']=True
      assert w.project('open-b',action='open',cwd=str(other))==opened
      assert w.project('open-b',action='list')['code']=='request_id_conflict';result['checks']['request_id_replay_and_conflict']=True
      w.send(type='cancel',id='foreign-cancel',target_id='slow-a');assert w.response('foreign-cancel')['code']=='project_mismatch'
      w.send(type='prompt',id='queued-b',text='QUEUED_B');assert not any(r['model']=='gpt-4o-mini' for r in requests)
      assert not w.project('busy-close',action='close',id=b)['success'];result['checks']['queued_owner_cannot_close_and_foreign_cancel_rejected']=True
      release.set();approval_a=w.wait(lambda v:v.get('event',{}).get('type')=='approval_request');assert approval_a['project']['project_id']==a
      w.send(type='approve',id='foreign-approval',approval_id=approval_a['event']['approval']['request_id'],decision='allow');assert not w.response('foreign-approval')['success'];assert not (root/'initial/marker.txt').exists();result['checks']['inactive_approval_keeps_owner_and_foreign_response_cannot_grant']=True
      assert w.project('select-a',action='select',id=a)['success'];w.send(type='approve',id='approve-a',approval_id=approval_a['event']['approval']['request_id'],decision='allow');assert w.response('approve-a')['success']
      w.response('slow-a');approval_b=w.wait(lambda v:v.get('event',{}).get('type')=='approval_request');assert approval_b['project']['project_id']==b
      assert w.project('select-b',action='select',id=b)['success'];w.send(type='approve',id='approve-b',approval_id=approval_b['event']['approval']['request_id'],decision='allow');assert w.response('approve-b')['success'];assert w.response('queued-b')['success']
      assert (root/'initial/marker.txt').read_text()=='SLOW_A' and (other/'marker.txt').read_text()=='QUEUED_B';assert [r['model'] for r in requests]==['gpt-4.1','gpt-4.1','gpt-4o-mini','gpt-4o-mini'];result['checks']['actual_http_models_and_tool_writes_stay_in_each_project']=True
      for v in w.records:
       rid=v.get('request_id',v.get('id'))
       if rid in ('slow-a','queued-b'):assert v['project']['project_id']==({'slow-a':a,'queued-b':b}[rid]),v
      result['checks']['every_late_event_and_terminal_retains_admission_owner']=True
      saved=w.project('before-invalid',action='list')['data']['workspace'];assert w.project('cancelled',action='open')['data']['cancelled']
      for rid,path in [('missing',root/'missing'),('bad-config',bad)]:assert not w.project(rid,action='open',cwd=str(path))['success']
      assert w.project('after-invalid',action='list')['data']['workspace']==saved;result['checks']['cancel_and_failed_prepare_preserve_valid_project']=True
      shared=root/'sessions/project-workspace.json';prior=shared.read_bytes()
      (root/'sessions').chmod(0o500)
      try:denied=w.project('permission-select',action='select',id=a)
      finally:(root/'sessions').chmod(0o700)
      assert not denied['success'] and shared.read_bytes()==prior
      assert w.project('after-permission',action='list')['data']['workspace']==saved
      result['checks']['actual_checkpoint_permission_failure_keeps_previous_owner']=True
      capacity_ids=[]
      for index in range(62):
       path=root/('capacity-'+str(index));path.mkdir();opened_capacity=w.project('capacity-'+str(index),action='open',cwd=str(path));assert opened_capacity['success'];capacity_ids.append(opened_capacity['project']['project_id'])
      full=w.project('capacity-full',action='list')['data']['workspace'];overflow=root/'overflow';overflow.mkdir()
      assert not w.project('capacity-overflow',action='open',cwd=str(overflow))['success']
      assert w.project('capacity-after',action='list')['data']['workspace']==full
      for index,pid in enumerate(capacity_ids):assert w.project('capacity-close-'+str(index),action='close',id=pid)['success']
      assert w.project('capacity-return',action='select',id=b)['success']
      result['checks']['actual_jsonl_64_project_ceiling_rejects_without_mutation']=True
      w.send(type='resources',id='escape',path='..');assert not w.response('escape')['success'];result['checks']['selected_project_inspection_rejects_escape']=True
      w.close();before=len(requests);w=Wire(root,env);restored=w.project('restart',action='list');assert restored['project']==opened['project'] and len(requests)==before;result['checks']['independent_jsonl_restart_restores_owner_without_provider_reissue']=True
      w.close()
      t=Terminal(binary,root,env);terminals.append(t);t.command('/status',b'gpt-4o-mini');shared=root/'sessions/project-workspace.json';t.wait(lambda:json.loads(shared.read_text())['workspace']['active']==b);result['checks']['production_tui_restores_jsonl_selection_and_actual_model']=True
      t.command('/project select '+a);t.wait(lambda:json.loads(shared.read_text())['workspace']['active']==a);t.command('/status',b'gpt-4.1');t.close();result['checks']['tui_select_updates_same_shared_owner_checkpoint']=True
      w=Wire(root,env);assert w.project('after-tui',action='list')['project']['project_id']==a and len(requests)==before;w.close();result['checks']['independent_jsonl_restores_tui_selection']=True
      result.update(passed=True,request_count=len(requests),jsonl_processes=len(processes),tui_processes=len(terminals))
    finally:
     release.set();evidence.parent.mkdir(parents=True,exist_ok=True);evidence.with_suffix('.jsonl.log').write_text('\n'.join(json.dumps(v) for w in processes for v in w.records)+'\n');evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals));evidence.write_text(json.dumps(result,indent=2)+'\n')
     for w in processes:
      if w.p.poll() is None:w.p.kill();w.p.wait()
     for t in terminals:cleanup_terminal(t)
     server.shutdown();server.server_close()
    return result


def run_bentobox(binary: Path, evidence: Path):
    result = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "checks": {}}
    terminals = []
    try:
        with TemporaryDirectory(prefix="bentobox-128-", dir=ROOT / ".ops") as raw:
            root = Path(raw)
            config = root / "fixture"
            child = root / "initial" / "child"
            for path in (child, root / "sessions", config):
                path.mkdir(parents=True)
            (config / "config.toml").write_text(
                'backend="openai"\nmodel="gpt-4.1"\n'
                'base_url="http://127.0.0.1:9/v1"\nwire_api="responses"\n'
                'requires_openai_auth=false\nmax_retries=0\n'
            )
            (config / "auth.json").write_text(json.dumps({"OPENAI_API_KEY": "bentobox-fixture"}))
            (config / "auth.json").chmod(0o600)
            env = {k: v for k, v in os.environ.items() if not k.startswith(("ZENPI_", "OPENAI_"))}
            env.update(ZENPI_HOME=str(config), TERM="xterm-256color", NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
            tabs_path = config / "project-tabs.json"
            layout_path = config / "layout.json"

            def tabs():
                return json.loads(tabs_path.read_text())

            def active_cwd():
                value = tabs()
                return value["metadata"][value["projects"][value["active"]]]["cwd"]

            def layout():
                value = tabs()
                active = value["projects"][value["active"]]
                model = next(p["layout"] for p in value["project_state"] if p["name"] == active)
                return {key: model[key] for key in ("ratios", "row_weights", "collapsed", "focused")}

            t = Terminal(binary, root, env)
            terminals.append(t)
            t.command("/layout save")
            t.wait(layout_path.exists)
            original = layout()
            t.write(b"\t")
            t.wait(lambda: layout()["focused"] == "project_conversation")
            t.write(b"\x1b[1;5B")
            t.wait(lambda: layout()["focused"] == "resources")
            t.write(b"\x1b[1;6C")
            t.wait(lambda: layout()["ratios"] != original["ratios"])
            result["checks"]["real_keyboard_focus_and_split_resize_persist"] = True
            t.expect(b"Resources")
            rows = screen_text(bytes(t.output)).decode().splitlines()
            row = next(index + 1 for index, line in enumerate(rows) if "┌ Resources" in line)
            t.write(f"\x1b[<0;10;{row}M\x1b[<32;10;{row + 2}M\x1b[<0;10;{row + 2}m".encode())
            t.wait(lambda: layout()["row_weights"] != original["row_weights"])
            result["checks"]["real_mouse_row_drag_persists_weights"] = True
            t.command("/pane collapse resources", b"pane collapsed: resources")
            t.command("/pane focus gantt", b"pane focused: gantt")
            t.wait(lambda: "resources" in layout()["collapsed"] and layout()["focused"] == "gantt")
            selected = layout()
            result["checks"]["collapse_and_focus_use_existing_bentobox_model"] = True

            draft = "resize draft 中文 e\u0301"
            t.write(draft.encode())
            t.wait(lambda: any(p["draft"]["input"] == draft for p in tabs()["project_state"]))
            sizes = [(1, 1), (40, 12), (100, 28), (180, 45), (140, 40)]
            for width, height in sizes:
                fcntl.ioctl(t.fd, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
                deadline = time.monotonic() + .4
                while time.monotonic() < deadline:
                    drain(t.fd, t.output, .03)
            t.expect(draft.encode())
            assert layout() == selected
            result["checks"]["responsive_resize_preserves_unicode_draft_and_layout"] = sizes

            # The fixture has exactly one child: plus, Right, Enter is three
            # observed gestures. This is not a claim about arbitrary directories.
            started = time.monotonic()
            t.write(b"\x1b[<0;139;1M\x1b[<0;139;1m")
            t.expect(b"Open project folder")
            t.write(b"\x1b[C")
            t.write(b"\r")
            t.wait(lambda: active_cwd() == str(child.resolve()))
            result["checks"]["plus_to_valid_project"] = {
                "gestures": ["click top +", "Right into sole immediate child", "Enter"],
                "count": 3, "elapsed_ms": round((time.monotonic() - started) * 1000),
                "fixture_condition": "exactly one immediate child directory",
            }
            t.command("/pane expand resources")
            t.command("/pane focus resources")
            t.wait(lambda: layout()["focused"] == "resources" and "resources" not in layout()["collapsed"])
            t.folder(root / "initial", mouse=False)
            t.wait(lambda: active_cwd() == str((root / "initial").resolve()))
            t.expect(draft.encode())
            t.wait(lambda: layout() == selected)
            result["checks"]["switching_real_projects_restores_each_layout_and_draft"] = True

            t.write(b"\x15")
            t.command("/pane focus browser", b"pane unavailable")
            assert layout() == selected
            t.write(b"\x15")
            t.command("/layout nonsense")
            drain(t.fd, t.output, .2)
            assert layout() == selected
            t.write(b"\x15")
            t.folder(root / "missing", mouse=False)
            t.expect(b"No such file")
            t.cancel_picker()
            assert active_cwd() == str((root / "initial").resolve()) and layout() == selected
            result["checks"]["unavailable_pane_invalid_layout_and_cancel_keep_valid_state"] = True
            t.write(draft.encode())
            t.wait(lambda: any(p["draft"]["input"] == draft for p in tabs()["project_state"]))
            t.close(preserve_draft=True)
            t = Terminal(binary, root, env)
            terminals.append(t)
            t.expect(draft.encode())
            assert layout() == selected and active_cwd() == str((root / "initial").resolve())
            result["checks"]["independent_restart_restores_draft_ratios_collapse_focus"] = True
            t.write(b"\x15")
            t.command("/pane expand resources", b"pane expanded: resources")
            t.wait(lambda: "resources" not in layout()["collapsed"])
            t.write(b"\x1b[48;5u")
            t.wait(lambda: layout()["ratios"] == original["ratios"] and layout()["focused"] is None)
            result["checks"]["expand_and_keyboard_reset_restore_default_layout"] = True
            t.close()
            t = Terminal(binary, root, env)
            terminals.append(t)
            assert layout()["ratios"] == original["ratios"] and layout()["focused"] is None
            assert "resources" not in layout()["collapsed"]
            t.close()
            result["checks"]["second_restart_keeps_post_restore_layout_edits"] = True
            result["checks"]["terminal_alternate_screen_restored"] = True
            result.update(status="passed", real_pty=True, tui_processes=len(terminals), request_count=0)
    except Exception as error:
        result.update(status="failed", error=str(error))
        raise
    finally:
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix(".pty.log").write_bytes(b"\nPROCESS\n".join(bytes(t.output) for t in terminals))
        evidence.write_text(json.dumps(result, indent=2) + "\n")
        for terminal in terminals:
            cleanup_terminal(terminal)
    return result


def run_compact(binary: Path, evidence: Path):
    requests=[];summaries=[];gate=threading.Event();release=threading.Event();terminals=[]
    class Handler(BaseHTTPRequestHandler):
     def log_message(self,*a):pass
     def do_POST(self):
      body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body);summary=body.get('metadata',{}).get('purpose')=='semantic_compaction'
      self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
      def emit(kind,**values):self.wfile.write(('data: '+json.dumps(dict(type=kind,**values))+'\n\n').encode());self.wfile.flush()
      try:
       emit('response.created',response={'id':f'compact-{len(requests)}','model':body['model']})
       if summary:
        summaries.append(body);content=body['input'][-1]['content'];data=json.loads(content if isinstance(content,str) else content[0]['text']);assert not body.get('tools');assert body['max_output_tokens']==1000
        if len(summaries)==1:
         gate.set()
         while not release.wait(.05):emit('response.output_text.delta',delta='')
        text=json.dumps(dict(goals=['Keep BentoBox'],constraints=['Keep original project cwd'],decisions=['Use shared owner'],progress=['Seed context'],pending_tasks=['verify restart'],critical_facts=['No unconfirmed work is complete'],read_files=data['read_files'],modified_files=data['modified_files'],unresolved_tools=data['unresolved_tools']))
       else:text='Seed answer '+('x'*6000) if len(requests)<=2 else 'CONTINUED_AFTER_COMPACT'
       emit('response.output_text.delta',delta=text)
       emit('response.completed',response={'id':f'compact-{len(requests)}','model':body['model'],'status':'completed','usage':{'input_tokens':250,'output_tokens':150,'total_tokens':400}})
      except (BrokenPipeError,ConnectionResetError):pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start();result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':{}}
    try:
     with TemporaryDirectory(prefix='compact-tui-',dir=Path('.ops').resolve()) as raw:
      root=Path(raw);config=root/'fixture'
      for d in [root/'initial',root/'sessions',config]:d.mkdir(parents=True)
      (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n[[model_overrides]]\nprovider="openai"\nid="compact-small"\nversion="fixture"\ncontext_window=6000\nmax_output_tokens=1000\ntext=true\ntools=true\nstreaming=true\n')
      (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'compact-fixture'}));(config/'auth.json').chmod(0o600)
      env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(config),TERM='xterm-256color',NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
      t=Terminal(binary,root,env);terminals.append(t);journal=root/'sessions/initial.jsonl';checkpoint=config/'project-tabs.json'
      def records():return [json.loads(line) for line in journal.read_text().splitlines()]
      def cps():return [r['event'] for r in records() if r.get('event',{}).get('type')=='semantic_checkpoint']
      def idle():t.wait(lambda:b'Ready' in screen_text(bytes(t.output)).splitlines()[1] or b'Interrupted' in screen_text(bytes(t.output)).splitlines()[1] or b'Context compacted' in screen_text(bytes(t.output)).splitlines()[1])
      for i in range(2):
       t.write(b'\x1b[200~'+('Keep BentoBox '+str(i)+' '+('z'*6000)).encode()+b'\x1b[201~\r');t.wait(lambda:len([r for r in records() if r.get('turn',{}).get('role')=='assistant'])==i+1);idle()
      t.command('/model compact-small',b'model selected: compact-small');t.command('/compact');t.wait(gate.is_set)
      t.write(b'draft while summary waits');t.wait(lambda:any(r.get('draft',{}).get('input')=='draft while summary waits' for r in json.loads(checkpoint.read_text())['project_state']));assert not cps();result['checks']['ui_edits_while_actual_summary_http_waits']=True
      t.write(b'\x03');t.expect(b'Interrupted');assert not cps();result['checks']['cancel_keeps_previous_context_checkpoint']=True;release.set();t.write(b'\x15');t.command('/compact');t.wait(lambda:len(cps())==1);idle();assert len(summaries)==2;result['checks']['actual_summary_durable_commit_via_worker']=True
      before=len(requests);t.close();t=Terminal(binary,root,env);terminals.append(t);assert len(requests)==before and len(cps())==1;result['checks']['restart_keeps_committed_summary_without_resend']=True
      t.command('verify restart');t.expect(b'CONTINUED_AFTER_COMPACT');assert 'Keep original project cwd' in json.dumps(requests[-1]);result['checks']['next_provider_receives_semantic_summary']=True
      t.close();result['checks']['terminal_restored']=True;result.update(passed=True,request_count=len(requests),summary_requests=len(summaries))
    finally:
     release.set();evidence.parent.mkdir(parents=True,exist_ok=True);evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals));evidence.write_text(json.dumps(result,indent=2)+'\n')
     for t in terminals:cleanup_terminal(t)
     server.shutdown();server.server_close()
    return result


def run_local_controls(binary: Path, evidence: Path):
    result = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "checks": {}}
    terminals, wire_records = [], []
    try:
        with TemporaryDirectory(prefix="local-controls-128-", dir=ROOT / ".ops") as raw:
            root = Path(raw)
            work, config = root / "initial", root / "fixture"
            for path in (work, config, root / "sessions"):
                path.mkdir(parents=True)
            subprocess.run(["git", "init", "--quiet", str(work)], check=True)
            (work / "note.txt").write_text("LOCAL_DIFF_MARKER\n")
            (config / "config.toml").write_text('backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:9/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config / "auth.json").write_text(json.dumps({"OPENAI_API_KEY": "local-controls-fixture"}))
            (config / "auth.json").chmod(0o600)
            env = {k: v for k, v in os.environ.items() if not k.startswith(("ZENPI_", "OPENAI_"))}
            env.update(ZENPI_HOME=str(config), ZENPI_DOMAIN_STORE=str(root / "domains.jsonl"), TERM="xterm-256color", NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
            journal = root / "sessions/initial.jsonl"
            t = Terminal(binary, root, env)
            terminals.append(t)
            t.command("/diff note.txt", b"LOCAL_DIFF_MARKER")
            t.command("/review note.txt")
            t.expect(b"LOCAL_DIFF_MARKER")
            result["checks"]["actual_git_diff_and_review_of_untracked_fixture"] = True
            t.command("/persona ESFP", b"Persona ESFP")
            t.wait(lambda: '"persona":"ESFP"' in journal.read_text())
            persona_before = journal.read_bytes()
            t.command("/persona invalid", b"Command rejected")
            assert journal.read_bytes() == persona_before
            t.write(b"\x15")
            result["checks"]["persona_persists_and_invalid_selection_is_rejected"] = True
            t.command("/plan local-plan :: inspect; test; publish", b"plan created:")
            domain_path = root / "domains.jsonl"
            t.wait(lambda: domain_path.exists() and "local-plan" in domain_path.read_text())
            before_plan = domain_path.read_bytes()
            t.command("/plan invalid", b"use `/plan")
            t.write(b"\x15")
            assert domain_path.read_bytes() == before_plan
            result["checks"]["plan_creates_durable_blueprint_and_bad_syntax_keeps_it"] = True
            before_clear = journal.read_bytes()
            t.command("/clear")
            t.wait(lambda: b"LOCAL_DIFF_MARKER" not in screen_text(bytes(t.output)))
            assert journal.read_bytes() == before_clear
            result["checks"]["clear_only_clears_view_and_preserves_durable_session"] = True
            other = root / "other"
            other.mkdir()
            subprocess.run(["git", "init", "--quiet", str(other)], check=True)
            (other / "note.txt").write_text("OTHER_PROJECT_DIFF_MARKER\n")
            t.folder(other)
            checkpoint = config / "project-tabs.json"
            def current_cwd():
                value = json.loads(checkpoint.read_text())
                return value["metadata"][value["projects"][value["active"]]]["cwd"]
            t.wait(lambda: current_cwd() == str(other.resolve()))
            t.command("!printf started > shell-started; sleep 3")
            t.expect(b"Approval required")
            t.write(b"y\r")
            t.wait(lambda: (other / "shell-started").exists())
            t.command("/diff note.txt")
            t.expect(b"Command requires an idle project", timeout=2)
            assert b"LOCAL_DIFF_MARKER" not in screen_text(bytes(t.output))
            t.wait(lambda: b"Local shell exit 0" in screen_text(bytes(t.output)).splitlines()[1])
            t.write(b"\t\r")
            t.expect(b"OTHER_PROJECT_DIFF_MARKER")
            assert b"LOCAL_DIFF_MARKER" not in screen_text(bytes(t.output))
            result["checks"]["busy_diff_preserves_draft_then_uses_selected_cwd_on_retry"] = True
            t.folder(work)
            t.wait(lambda: current_cwd() == str(work.resolve()))
            t.close()

            def batch(commands):
                requests = [dict(schema_version=2, type="command", id=rid, text=text) for rid, text in commands]
                requests.append(dict(schema_version=2, type="shutdown", id="shutdown-" + commands[0][0]))
                completed = subprocess.run([str(binary), "--mode", "headless", "--session", str(journal)], cwd=work, env=env,
                    input="".join(json.dumps(r) + "\n" for r in requests), text=True, capture_output=True, timeout=30)
                assert completed.returncode == 0, completed.stderr
                records = [json.loads(line) for line in completed.stdout.splitlines()]
                wire_records.extend(records)
                responses = {r["id"]: r for r in records if r.get("type") == "response"}
                for rid, _ in commands:
                    assert responses[rid]["success"], responses[rid]
                return responses

            responses = batch([("persona-restart", "/persona"), ("blueprint-restart", "/blueprint show"),
                ("diff-jsonl", "/diff note.txt"), ("review-jsonl", "/review note.txt")])
            assert "ESFP" in json.dumps(responses["persona-restart"])
            assert "local-plan" in json.dumps(responses["blueprint-restart"])
            for rid in ("diff-jsonl", "review-jsonl"):
                assert "LOCAL_DIFF_MARKER" in json.dumps(responses[rid])
            result["checks"]["independent_jsonl_restores_tui_persona_plan_and_diff_owner"] = True
            fork = root / "sessions/fork.jsonl"
            batch([("fork", f"/session fork {journal} {fork}"), ("agents", "/session agents")])
            assert fork.exists() and fork.stat().st_size > 0
            result["checks"]["actual_jsonl_session_fork_and_agent_registry"] = True
            events = [json.loads(line) for line in journal.read_text().splitlines()]
            assert not any(record.get("event", {}).get("operation_kind") == "provider" for record in events)
            result.update(status="passed", request_count=0, tui_processes=1, jsonl_processes=2)
    except Exception as error:
        result.update(status="failed", error=str(error))
        raise
    finally:
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix(".pty.log").write_bytes(b"\nPROCESS\n".join(bytes(t.output) for t in terminals))
        evidence.with_suffix(".jsonl.log").write_text("".join(json.dumps(r) + "\n" for r in wire_records))
        evidence.write_text(json.dumps(result, indent=2) + "\n")
        for terminal in terminals:
            cleanup_terminal(terminal)
    return result


CASES = {
    "shared-projects": None,
    "bentobox": None,
    "compact": None,
    "local-controls": None,
    "projects": "tui_project_workspace_smoke.py",
    "composer": "tui_composer_smoke.py",
    "ordinary-paste": "tui_composer_smoke.py",
    "large-paste": "tui_composer_smoke.py",
    "queued-shell-paste": "tui_composer_smoke.py",
    "kill-yank": "tui_composer_smoke.py",
    "project-checkpoints": "tui_project_workspace_smoke.py",
    "tui-session-resume": "tui_project_workspace_smoke.py",
    "commands": "tui_command_palette_smoke.py",
    "approval": "tui_approval_smoke.py",
    "transcript": "tui_transcript_ux_smoke.py",
}


CASE_ARGS = {
    "ordinary-paste": ["--burst-only"],
    "large-paste": ["--large-paste-only"],
    "queued-shell-paste": ["--queued-shell-paste-only"],
    "kill-yank": ["--kill-yank-only"],
    "project-checkpoints": ["--checkpoint-only"],
    "tui-session-resume": ["--resume-only"],
}


def validated_check_count(result):
    checks = result.get("checks", result.get("assertions"))
    if not isinstance(checks, (dict, list)) or not checks:
        raise ValueError("case has no behavioral checks")
    values = checks.values() if isinstance(checks, dict) else checks
    if not all(values):
        raise ValueError("case contains a failed or empty behavioral check")
    return len(checks)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--report-dir", "--evidence-dir", dest="evidence_dir", type=Path)
    parser.add_argument("--case", choices=["all", *CASES], default="all")
    parser.add_argument("--evidence", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    (ROOT / ".ops").mkdir(exist_ok=True)
    if args.evidence:
        handlers = {"shared-projects": run_shared_projects, "bentobox": run_bentobox, "compact": run_compact, "local-controls": run_local_controls}
        assert args.case in handlers
        handler = handlers[args.case]
        print(json.dumps(handler(binary, args.evidence.resolve())))
        return
    if not args.evidence_dir:
        parser.error("--evidence-dir is required")
    evidence_dir = args.evidence_dir.resolve()
    evidence_dir.mkdir(parents=True, exist_ok=False)
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    support_paths = {Path(__file__).resolve(), ROOT / "tools/tui_user_shell_smoke.py"}
    support_paths.update(ROOT / "tools" / script for script in CASES.values() if script)
    support_hashes = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
                      for path in sorted(support_paths)}
    report = {"status": "running", "binary": str(binary), "binary_sha256": digest,
              "binary_bytes": binary.stat().st_size, "platform": platform.platform(),
              "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "support_sha256": support_hashes, "cases": {}}
    selected = list(CASES) if args.case == "all" else [args.case]
    try:
        for case in selected:
            script = ROOT / "tools" / (CASES[case] or Path(__file__).name)
            result_path = evidence_dir / (case + ".json")
            command = [sys.executable, str(script), "--binary", str(binary), "--evidence", str(result_path)]
            if CASES[case] is None:
                command += ["--case", case]
            command += CASE_ARGS.get(case, [])
            start = time.monotonic()
            print("Running " + case, flush=True)
            with (evidence_dir / (case + ".log")).open("w") as output:
                completed = subprocess.run(command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT, timeout=240)
            assert completed.returncode == 0, f"{case} failed; inspect {case}.log"
            result = json.loads(result_path.read_text())
            assert result.get("status") == "passed" or result.get("passed") is True, case
            check_count = validated_check_count(result)
            assert result.get("binary_sha256") == digest, f"{case} binary identity mismatch"
            assert hashlib.sha256(binary.read_bytes()).hexdigest() == digest, "binary changed during run"
            for path, expected in support_hashes.items():
                assert hashlib.sha256((ROOT / path).read_bytes()).hexdigest() == expected, f"helper changed during run: {path}"
            report["cases"][case] = {"status": "passed", "duration_seconds": round(time.monotonic() - start, 3),
                "script_sha256": hashlib.sha256(script.read_bytes()).hexdigest(), "command": command,
                "result_sha256": hashlib.sha256(result_path.read_bytes()).hexdigest(),
                "check_count": check_count}
        report["status"] = "passed"
    except Exception as error:
        report.update(status="failed", error=str(error))
        raise
    finally:
        (evidence_dir / "manifest.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()

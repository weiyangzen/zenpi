#!/usr/bin/env python3
"""Real release PTY: directory picker, project config/cwd, late shell output and restart."""
from __future__ import annotations
import argparse, fcntl, hashlib, json, os, pty, signal, struct, termios, time, re, unicodedata
from pathlib import Path
from tempfile import TemporaryDirectory
from tui_user_shell_smoke import drain, send, visible
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def resume_smoke(binary, evidence):
    """Production TUI resume failure/retry/restart against shared owner storage."""
    import subprocess, queue
    terminals, jsonl, http_requests = [], [], []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            http_requests.append(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
            for kind, payload in [('response.output_text.delta',{'delta':'resume target fixture reply'}),('response.completed',{'response':{'id':'resume-seed','model':'gpt-4.1','status':'completed'}})]:
                self.wfile.write(('event: '+kind+'\ndata: '+json.dumps({'type':kind,**payload})+'\n\n').encode());self.wfile.flush()
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}}
    try:
        for fault in ('permission', 'stale-writer'):
            for entry in ('open', 'last', 'control'):
                with TemporaryDirectory(prefix='tui-resume-', dir=Path('.ops').resolve()) as raw:
                    root=Path(raw); config=root/'fixture'; sessions=root/'sessions'; other=root/'other'
                    target=(config/'sessions/latest.jsonl') if entry=='last' else sessions/'alternate.jsonl'
                    for path in (root/'initial', sessions, config, target.parent, other): path.mkdir(parents=True, exist_ok=True)
                    (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
                    (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'tui-resume-fixture'})); (config/'auth.json').chmod(0o600)
                    env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))}; env.update(ZENPI_HOME=str(config), TERM='xterm-256color')
                    def headless(session, requests):
                        values=requests+[{'type':'shutdown'}]
                        process=subprocess.Popen([str(binary),'--mode','headless','--session',str(session)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,cwd=root/'initial',env=env,bufsize=1)
                        records=[];incoming=queue.Queue();responses=[]
                        def read():
                            for line in process.stdout: records.append(line);incoming.put(json.loads(line))
                            incoming.put(None)
                        threading.Thread(target=read,daemon=True).start()
                        try:
                            for i,value in enumerate(values):
                                process.stdin.write(json.dumps(dict(schema_version=2,id=str(i),**value))+'\n');process.stdin.flush()
                                while True:
                                    event=incoming.get(timeout=15);assert event is not None,'headless closed early'
                                    if event.get('type')=='response' and event.get('id')==str(i):
                                        assert event['success'],event
                                        responses.append(event);break
                            process.stdin.close();assert process.wait(timeout=5)==0
                        finally:
                            if process.poll() is None:process.kill();process.wait(timeout=5)
                            jsonl.append({'fault':fault,'entry':entry,'stdout':''.join(records),'stderr':process.stderr.read(),'exit_code':process.returncode})
                        return responses
                    seed=[{'type':'command','text':'/persona ISTJ'}]
                    if entry=='control':seed.append({'type':'prompt','text':'resume-target-marker'})
                    headless(target,seed)
                    t=Terminal(binary,root,env); terminals.append(t); checkpoint=config/'project-tabs.json'; shared=sessions/'project-workspace.json'
                    def saved(): return json.loads(checkpoint.read_text())
                    def active():
                        value=saved(); return next(row for row in value['project_state'] if row['name']==value['projects'][value['active']])
                    t.command('/persona ESFP'); t.command('/pane collapse resources')
                    if entry=='control':
                        t.command('/session search resume-target-marker')
                        t.command('/pane collapse replay_controls')
                    t.wait(lambda: checkpoint.exists() and shared.exists() and ('replay_controls' if entry=='control' else 'resources') in active()['layout']['collapsed'] and (entry!='control' or active()['layout']['focused']=='session_list'))
                    original=active(); old_id=original['name']; old_meta=original['metadata']; layout=original['layout']; old_session=saved()['session_cursors'][old_id]['session_id']; before_target=target.read_bytes()
                    command='/session resume-last' if entry=='last' else '/session open '+str(target)
                    if fault=='stale-writer':
                        headless(sessions/'writer.jsonl',[{'type':'project','project':{'action':'open','cwd':str(other)}}])
                    before=shared.read_bytes()
                    if fault=='permission': sessions.chmod(0o500)
                    try:
                        if entry=='control': t.write(b'\r')
                        else: t.command(command)
                        t.wait(lambda:'session open failed:' in json.dumps(active()))
                        assert active()['metadata']==old_meta
                        assert active()['layout']==layout
                        assert active()['draft']['input']==('' if entry=='control' else command)
                        assert saved()['session_cursors'][old_id]['session_id']==old_session
                        assert shared.read_bytes()==before
                        assert target.read_bytes()==before_target
                        result['checks'][fault+'-'+entry+'-failure_keeps_owner_draft_layout_checkpoint']=True
                    finally: sessions.chmod(0o700)
                    if fault=='permission':
                        t.write(b'\x15')
                        if entry=='control': t.write(b'\r')
                        else: t.command(command)
                        t.wait(lambda: active()['metadata']['session_path']==str(target))
                        assert active()['layout']==layout
                        assert saved()['session_cursors'][old_id]['session_id']!=old_session
                        assert json.loads(shared.read_text())['sessions'][old_id]==str(target)
                        expected=str(root/'initial')
                    else: expected=str(other)
                    t.close(preserve_draft=True)
                    t=Terminal(binary,root,env);terminals.append(t)
                    t.wait(lambda: active()['metadata']['cwd']==expected)
                    if fault=='permission':
                        assert active()['metadata']['session_path']==str(target)
                    else:
                        old_view=next(row for row in saved()['project_state'] if row['name']==old_id)
                        assert old_view['draft']['input']==('' if entry=='control' else command)
                        assert old_view['layout']==layout, {'restored_layout':old_view['layout'],'expected_layout':layout}
                    result['checks'][fault+'-'+entry+'-independent_restart_uses_committed_owner']=True
                    t.close(preserve_draft=True)
        result.update(status='passed',tui_processes=len(terminals),headless_processes=len(jsonl),http_requests=len(http_requests))
    except Exception as error:
        result.update(status='failed',error=str(error));raise
    finally:
        evidence.parent.mkdir(parents=True,exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.jsonl.log').write_text(json.dumps(jsonl,ensure_ascii=False,indent=2)+'\n')
        evidence.with_suffix('.http.json').write_text(json.dumps(http_requests,ensure_ascii=False,indent=2)+'\n')
        evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
        for t in terminals:t.cleanup()
        server.shutdown();server.server_close()
    return result

def checkpoint_smoke(binary, evidence):
    """Gate actual provider deltas after keyboard checkpointing has settled."""
    gates = [{name: threading.Event() for name in ('first', 'second', 'finish')} for _ in range(2)]
    requests, terminals = [], []
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}}
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            index = len(requests)
            requests.append(body)
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            def emit(delta=None, finish=None):
                value = {'id': f'checkpoint-{index}', 'model': body['model'],
                         'choices': [{'index': 0, 'delta': delta or {}, 'finish_reason': finish}]}
                self.wfile.write(('data: ' + json.dumps(value) + '\n\n').encode())
                self.wfile.flush()
            try:
                emit({'role': 'assistant'})
                assert gates[index]['first'].wait(20)
                emit({'reasoning_content': f'REASON_{index}_FIRST '})
                emit({'content': f'ANSWER_{index}_FIRST '})
                assert gates[index]['second'].wait(20)
                emit({'reasoning_content': f'REASON_{index}_SECOND '})
                emit({'content': f'ANSWER_{index}_SECOND '})
                assert gates[index]['finish'].wait(20)
                emit({'content': f'ANSWER_{index}_FINAL'})
                emit(finish='stop')
                self.wfile.write(b'data: [DONE]\n\n')
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                pass
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with TemporaryDirectory(prefix='checkpoint-pty-', dir=Path('.ops').resolve()) as raw:
            root = Path(raw)
            config, other = root/'fixture', root/'other'
            for path in (root/'initial', root/'sessions', config, other):
                path.mkdir(parents=True)
            (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="chat"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'checkpoint-fixture'}))
            (config/'auth.json').chmod(0o600)
            env = {k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
            env.update(ZENPI_HOME=str(config), TERM='xterm-256color')
            checkpoint = config/'project-tabs.json'
            def saved():
                return json.loads(checkpoint.read_text())
            def state_for(path):
                return next(p for p in saved()['project_state'] if p['metadata']['cwd'] == str(path.resolve()))
            def saved_text(path):
                return json.dumps(state_for(path)['messages'])
            def settle(t):
                deadline = time.monotonic() + .6
                while time.monotonic() < deadline:
                    drain(t.fd, t.output, .03)
            def advance(t, index, name, path):
                gates[index][name].set()
                suffix = {'first': 'FIRST', 'second': 'SECOND', 'finish': 'FINAL'}[name]
                marker = f'ANSWER_{index}_{suffix}'
                if index == 0:
                    t.expect(marker.encode())
                t.wait(lambda: marker in saved_text(path), timeout=3)
                if name != 'finish':
                    assert f'REASON_{index}_{suffix}' in saved_text(path)
                result['checks'][f'{"active" if index == 0 else "background"}_{name}_checkpoint_without_keyboard'] = True
            t = Terminal(binary, root, env)
            terminals.append(t)
            t.command('persist-active-stream')
            t.wait(lambda: len(requests) == 1)
            settle(t)
            advance(t, 0, 'first', root/'initial')
            settle(t)
            advance(t, 0, 'second', root/'initial')
            settle(t)
            advance(t, 0, 'finish', root/'initial')
            t.wait(lambda: b'Ready' in screen_text(bytes(t.output)).splitlines()[1])
            # Capture current durable bytes before any shutdown signal; a final
            # flush must not be able to disguise the absence of periodic saves.
            result['active_pre_shutdown_checkpoint_sha256'] = hashlib.sha256(checkpoint.read_bytes()).hexdigest()
            t.close(preserve_draft=True)
            t = Terminal(binary, root, env)
            terminals.append(t)
            t.expect(b'ANSWER_0_FINAL')
            assert len(requests) == 1
            result['checks']['independent_restart_restores_active_stream_without_reissue'] = True

            t.command('persist-background-stream')
            t.wait(lambda: len(requests) == 2)
            t.folder(other)
            t.wait(lambda: saved()['metadata'][saved()['projects'][saved()['active']]]['cwd'] == str(other.resolve()))
            t.command('/pane collapse resources')
            t.command('/pane focus gantt')
            t.write(b'background-safe draft')
            t.wait(lambda: state_for(other)['draft']['input'] == 'background-safe draft')
            t.wait(lambda: state_for(other)['layout']['focused'] == 'gantt')
            other_layout = state_for(other)['layout']
            settle(t)
            advance(t, 1, 'first', root/'initial')
            settle(t)
            advance(t, 1, 'second', root/'initial')
            settle(t)
            advance(t, 1, 'finish', root/'initial')
            assert 'ANSWER_1' not in saved_text(other)
            assert state_for(other)['draft']['input'] == 'background-safe draft'
            assert state_for(other)['layout'] == other_layout
            result['checks']['background_updates_preserve_visible_project_draft_and_layout'] = True
            result['background_pre_shutdown_checkpoint_sha256'] = hashlib.sha256(checkpoint.read_bytes()).hexdigest()
            t.close(preserve_draft=True)
            t = Terminal(binary, root, env)
            terminals.append(t)
            t.expect(b'background-safe draft')
            assert state_for(other)['layout'] == other_layout
            assert len(requests) == 2
            t.folder(root/'initial')
            t.expect(b'ANSWER_1_FINAL')
            result['checks']['independent_restart_restores_background_result_and_visible_layout'] = True
            t.close()
            result.update(status='passed', request_count=len(requests), tui_processes=len(terminals))
    except Exception as error:
        result.update(status='failed', error=str(error))
        raise
    finally:
        for gate in gates:
            for event in gate.values():
                event.set()
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.write_text(json.dumps(result, indent=2)+'\n')
        for terminal in terminals:
            terminal.cleanup()
        server.shutdown()
        server.server_close()
    return result

def screen_text(data,width=140,height=40,left_only=False):
    """Reconstruct the cursor-addressed cells Ratatui writes over a real PTY.

    Support CSI cursor/erase and OSC controls, including wide Unicode cells;
    styles do not change text. This is an assertion aid, not a replacement UI.
    """
    rows=[[' ']*width for _ in range(height)];x=y=0;i=0
    text=data.decode('utf-8',errors='replace')
    while i<len(text):
        char=text[i]
        if char=='\x1b':
            match=re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])',text[i:])
            if match:
                raw,_,command=match.groups();i+=len(match.group());parts=raw.lstrip('?').split(';');values=[int(p) if p.isdigit() else 0 for p in parts];n=values[0] or 1
                if command in 'Hf':y=max(0,min(height-1,n-1));x=max(0,min(width-1,(values[1] if len(values)>1 and values[1] else 1)-1))
                elif command=='G':x=max(0,min(width-1,n-1))
                elif command=='d':y=max(0,min(height-1,n-1))
                elif command=='A':y=max(0,y-n)
                elif command=='B':y=min(height-1,y+n)
                elif command=='C':x=min(width-1,x+n)
                elif command=='D':x=max(0,x-n)
                elif command=='J' and values[0] in (2,3):rows=[[' ']*width for _ in range(height)]
                elif command=='K':
                    start,end=(0,width) if values[0]==2 else ((0,x+1) if values[0]==1 else (x,width));rows[y][start:end]=[' ']*(end-start)
                elif command=='h' and raw=='?1049':rows=[[' ']*width for _ in range(height)];x=y=0
                continue
            match=re.match(r'\x1b\][^\a]*(?:\a|\x1b\\)',text[i:])
            if match:i+=len(match.group());continue
            i+=2;continue
        if char=='\r':x=0
        elif char=='\n':y=min(height-1,y+1)
        elif char=='\b':x=max(0,x-1)
        elif ord(char)>=32:
            cells=0 if unicodedata.combining(char) else (2 if unicodedata.east_asian_width(char) in ('W','F') else 1)
            if cells==0 and x>0:rows[y][x-1]+=char
            elif cells and x<width:
                rows[y][x]=char
                if cells==2 and x+1<width:rows[y][x+1]=''
                x+=cells
        i+=1
    return '\n'.join(''.join(row[:56] if left_only else row) for row in rows).encode()

class Terminal:
    def __init__(self,binary,root,env):
        self.output=bytearray();self.fd=None;self.pid=None
        self.checkpoint=Path(env['ZENPI_HOME'])/'project-tabs.json'
        pid,fd=pty.fork()
        if pid==0:
            os.chdir(root/'initial')
            os.execve(str(binary),[str(binary),'--mode','tui','--backend','openai','--session',str(root/'sessions/initial.jsonl')],env)
        self.pid,self.fd=pid,fd
        fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',40,140,0,0))
        self.expect(b'Prompt')
    def expect(self,text,timeout=12,start=0):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            drain(self.fd,self.output,.05)
            normalize=lambda value: ''.join(c for c in value.decode(errors='replace').lower() if not c.isspace() and c not in '│─┌┐└┘●').encode()
            if any(normalize(text) in normalize(screen_text(bytes(self.output),left_only=left)) for left in (False,True)):return
        raise AssertionError(f'missing {text!r}: {screen_text(bytes(self.output)).decode(errors="replace")}')
    def wait(self,predicate,timeout=12):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            drain(self.fd,self.output,.05)
            try:
                value=predicate()
                if value:return value
            except (FileNotFoundError,json.JSONDecodeError):pass
        raise AssertionError(f'condition timed out: {visible(bytes(self.output[-7000:]))!r}')
    def write(self,data):send(self.fd,data)
    def draft(self):
        value=json.loads(self.checkpoint.read_text())
        active=value['projects'][value['active']]
        return next(row['draft']['input'] for row in value['project_state'] if row['name']==active)
    def clear_input(self):
        # Composer Ctrl-U is logical-line kill. Explicitly visit buffer start
        # and kill each visible line plus its LF; hidden fold LFs only add no-ops.
        # Picker/filter fields retain their own single-line Ctrl-U semantics.
        count = 2 * self.draft().count('\n') + 1
        self.write(b'\x1b[1;5H' + b'\x0b' * count)
        self.wait(lambda: self.draft() == '')
    def cancel_picker(self):
        self.write(b'\x1b')
        deadline=time.monotonic()+0.2
        while time.monotonic()<deadline:drain(self.fd,self.output,.02)
    def folder(self,path,mouse=True):
        mark=len(self.output)
        self.write(b'\x1b[<0;139;1M\x1b[<0;139;1m' if mouse else b'\x14')
        self.expect(b'Open project folder',start=mark)
        self.write(b'\x15\x1b[200~'+str(path).encode()+b'\x1b[201~\r')
    def type_text(self,text):
        # Model separate physical keystrokes when a scenario tests live menus.
        for char in text:
            self.write(char.encode())
            deadline=time.monotonic()+0.035
            while time.monotonic()<deadline:drain(self.fd,self.output,.005)
    def deliberate_enter(self):
        # Ordinary bytes written in one batch are paste-like. A command helper
        # models the later deliberate Enter, after the 120ms paste window.
        deadline=time.monotonic()+0.15
        while time.monotonic()<deadline:drain(self.fd,self.output,.01)
        self.write(b'\r')
    def command(self,text,expect=None):
        mark=len(self.output);self.write(text.encode())
        self.wait(lambda:self.draft().endswith(text))
        self.deliberate_enter()
        if expect:self.expect(expect,start=mark)
    def shell(self,command):
        mark=len(self.output);self.write(('!'+command).encode())
        self.wait(lambda:self.draft().endswith('!'+command))
        self.deliberate_enter()
        self.wait(lambda:b'Approvalrequired' in b''.join(screen_text(bytes(self.output)).splitlines()[1].split()))
        self.write(b'y\r')
        self.wait(lambda:b'Approvalrequired' not in b''.join(screen_text(bytes(self.output)).splitlines()[1].split()))
    def close(self,preserve_draft=False):
        if self.pid is None:return
        if preserve_draft:os.kill(self.pid,signal.SIGTERM)
        else:
            self.clear_input()
            self.write(b'/quit')
            self.wait(lambda:self.draft()=='/quit')
            self.deliberate_enter()
        deadline=time.monotonic()+8
        while time.monotonic()<deadline:
            drain(self.fd,self.output,.05)
            waited,status=os.waitpid(self.pid,os.WNOHANG)
            if waited:
                self.pid=None;os.close(self.fd)
                assert os.WIFEXITED(status) and os.WEXITSTATUS(status)==0,status
                assert b'\x1b[?1049l' in self.output,'alternate screen not restored'
                return
        raise AssertionError('TUI did not exit')
    def cleanup(self):
        if self.pid:
            # Closing the master first releases pending terminal output on
            # macOS. A blocking wait with the master open can hang teardown.
            os.close(self.fd);self.fd=None
            try:os.kill(self.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            deadline=time.monotonic()+3
            while time.monotonic()<deadline:
                try:
                    if os.waitpid(self.pid,os.WNOHANG)[0]:self.pid=None;return
                except ChildProcessError:self.pid=None;return
                time.sleep(.01)
            raise RuntimeError(f'fixture child {self.pid} did not exit after SIGKILL')

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True,type=Path);parser.add_argument('--evidence',type=Path,default=Path('Docs/quality/stage1/ux-pty-result.json'));parser.add_argument('--checkpoint-only',action='store_true');parser.add_argument('--resume-only',action='store_true');args=parser.parse_args();binary=args.binary.resolve()
    if args.resume_only:
        print(json.dumps(resume_smoke(binary,args.evidence)));return
    if args.checkpoint_only:
        Path('.ops').mkdir(exist_ok=True)
        print(json.dumps(checkpoint_smoke(binary,args.evidence)))
        return
    ops=Path('.ops');ops.mkdir(exist_ok=True)
    with TemporaryDirectory(prefix='project-pty-',dir=ops.resolve()) as raw:
        root=Path(raw);first=root/'one/工作 folder';second=root/'two/工作 folder'
        for path in [root/'initial',root/'home/.zenpi',root/'sessions',first/'.zenpi',second/'.zenpi']:path.mkdir(parents=True,exist_ok=True)
        (root/'home/.zenpi/config.toml').write_text('backend="openai"\nmodel="initial-model"\nbase_url="http://127.0.0.1:9/v1"\nwire_api="responses"\nrequires_openai_auth=false\n')
        # Explicit fixture auth keeps fallback discovery from reading user credentials.
        (root/'home/.zenpi/auth.json').write_text(json.dumps({'OPENAI_API_KEY':'project-pty-fixture-key'}))
        (root/'home/.zenpi/auth.json').chmod(0o600)
        (first/'.zenpi/config.toml').write_text('model="first-project-model"\n')
        (second/'.zenpi/config.toml').write_text('model="second-project-model"\n')
        env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))}
        env.update(ZENPI_HOME=str(root/'home/.zenpi'),TERM='xterm-256color')
        checkpoint=root/'home/.zenpi/project-tabs.json'
        def saved():return json.loads(checkpoint.read_text())
        def active_cwd():
            value=saved();return value['metadata'][value['projects'][value['active']]]['cwd']
        terminals=[]
        try:
            terminal=Terminal(binary,root,env);terminals.append(terminal)
            terminal.wait(lambda:checkpoint.exists());initial=saved();assert len(initial['projects'])==1
            mark=len(terminal.output);terminal.write(b'\x1b[<0;139;1M\x1b[<0;139;1m');terminal.expect(b'Open project folder',start=mark);terminal.cancel_picker()
            # Next control chord also proves Esc returned input focus.
            terminal.folder(root/'missing',mouse=False);terminal.expect(b'No such file');terminal.cancel_picker()
            terminal.wait(lambda:len(saved()['projects'])==1)
            terminal.folder(first);terminal.wait(lambda:active_cwd()==str(first.resolve()))
            terminal.command('/status',b'first-project-model')
            terminal.shell('printf first > marker.txt');terminal.wait(lambda:(first/'marker.txt').exists())
            terminal.shell('sleep 2; printf late-first > late.txt; echo first-project-late-output')
            terminal.folder(second);terminal.wait(lambda:active_cwd()==str(second.resolve()))
            terminal.command('/status',b'second-project-model')
            terminal.wait(lambda:(first/'late.txt').exists())
            terminal.shell('printf second > marker.txt');terminal.wait(lambda:(second/'marker.txt').exists())
            terminal.wait(lambda:any('first-project-late-output' in str(p['messages']) for p in saved()['project_state'] if p['metadata']['cwd']==str(first.resolve())))
            data=saved();first_state=next(p for p in data['project_state'] if p['metadata']['cwd']==str(first.resolve()));second_state=next(p for p in data['project_state'] if p['metadata']['cwd']==str(second.resolve()))
            assert 'first-project-late-output' not in str(second_state['messages']),'late output leaked into second project'
            assert (first/'marker.txt').read_text()=='first' and (second/'marker.txt').read_text()=='second'
            assert not (second/'late.txt').exists() and not (root/'initial/marker.txt').exists()
            for entry in [first_state,second_state]:
                records=[json.loads(line) for line in Path(entry['metadata']['session_path']).read_text().splitlines()]
                assert records[0]['cwd']==entry['metadata']['cwd']
                assert not any(r.get('event',{}).get('operation_kind')=='provider' for r in records),'shell submitted a model request'
            # Alias activation and inactive close go through the same host.
            terminal.folder(first/'../工作 folder',mouse=False);terminal.wait(lambda:active_cwd()==str(first.resolve()));assert len(saved()['projects'])==3
            terminal.write(b'draft survives restart')
            terminal.wait(lambda:any(p['name']==saved()['projects'][saved()['active']] and p['draft']['input']=='draft survives restart' for p in saved()['project_state']))
            terminal.close(preserve_draft=True)
            restart=Terminal(binary,root,env);terminals.append(restart);restart.expect(b'draft survives restart')
            assert active_cwd()==str(first.resolve())
            restart.write(b'\x15');restart.command('/status',b'first-project-model')
            # Keyboard project select retains the same real owner after restart.
            restart.write(b'\x15');restart.command('/project select '+second_state['name'])
            restart.wait(lambda:active_cwd()==str(second.resolve()));restart.command('/status',b'second-project-model')
            restart.close()
            result={'status':'passed','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'real_pty':True,'assertions':['top-row mouse plus opens picker','Esc and invalid path create no placeholder','Unicode/space paths and aliases','distinct equal basenames','project config changes real backend model','real shell writes in two selected roots','session headers match tools cwd','switch while shell is running','late output stays with source project','restart preserves selected runtime and draft','slash selects same owner','terminal restored']}
            args.evidence.parent.mkdir(parents=True,exist_ok=True);args.evidence.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
        except Exception:
            for index,t in enumerate(terminals):
                path=args.evidence.with_suffix(f'.failure-{index}.txt');path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(bytes(t.output))
            raise
        finally:
            for terminal in terminals:terminal.cleanup()
if __name__=='__main__':main()

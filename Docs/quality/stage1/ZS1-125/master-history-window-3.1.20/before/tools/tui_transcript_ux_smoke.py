#!/usr/bin/env python3
"""Actual Chat SSE -> release TUI PTY transcript controls and terminal restoration."""
from __future__ import annotations
import argparse, base64, fcntl, hashlib, json, os, re, struct, termios, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from tui_project_workspace_smoke import Terminal, screen_text
from tui_user_shell_smoke import drain

REASONING='REASONING_STREAM_MARKER'
PARTIAL='ANSWER_START\n' + ''.join(f'ROW_{i:03}\n' for i in range(20))
REMAINDER=''.join(f'ROW_{i:03}\n' for i in range(20,40))+'```rust\nfn copied_block() {}\n```\nANSWER_END'
ANSWER=PARTIAL+REMAINDER

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True,type=Path);parser.add_argument('--evidence',type=Path,default=Path('Docs/quality/stage1/ux125-pty-result.json'));args=parser.parse_args();binary=args.binary.resolve()
    phase1=threading.Event();phase2=threading.Event();requests=[]
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            value=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(value)
            assert value['stream'] is True, 'fixture model must explicitly negotiate streaming'
            latest=next((m.get('content','') for m in reversed(value['messages']) if m['role']=='user'),'')
            if 'error-case' in str(latest):
                self.send_response(500);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps({'error':{'message':'INSPECT_ERROR_DETAIL provider fixture rejected request','type':'fixture_error'}}).encode());return
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
            def emit(delta=None,finish=None,usage=None):
                body={'id':'chatcmpl-ux125','model':'ux-served','choices':[] if usage else [{'index':0,'delta':delta or {},'finish_reason':finish}]}
                if usage:body['usage']=usage
                self.wfile.write(('data: '+json.dumps(body)+'\n\n').encode());self.wfile.flush()
            try:
                if 'slow-case' in str(latest):
                    for i in range(100):emit({'content':f'SLOW_{i:03}\n'});time.sleep(.1)
                else:
                    emit({'reasoning_content':REASONING})
                    if not phase1.wait(20):return
                    emit({'content':PARTIAL})
                    if not phase2.wait(20):return
                    emit({'content':REMAINDER})
                emit(finish='stop');emit(usage={'prompt_tokens':123,'completion_tokens':45,'total_tokens':168});self.wfile.write(b'data: [DONE]\n\n');self.wfile.flush()
            except (BrokenPipeError,ConnectionResetError):pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    Path('.ops').mkdir(exist_ok=True)
    result={'checks':{},'binary':str(binary),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
    terminal=None
    try:
        with TemporaryDirectory(prefix='transcript-pty-',dir=Path('.ops').resolve()) as raw:
            root=Path(raw)
            for path in [root/'initial',root/'other',root/'sessions',root/'fixture']:path.mkdir(parents=True)
            (root/'fixture/config.toml').write_text(f'backend="openai"\nmodel="ux-fixture"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="chat"\nrequires_openai_auth=false\nmax_retries=0\n[[model_overrides]]\nprovider="openai"\nid="ux-fixture"\nversion="ux125-sse-fixture-v1"\nstreaming=true\n')
            (root/'fixture/auth.json').write_text(json.dumps({'OPENAI_API_KEY':'ux125-test-fixture'}));(root/'fixture/auth.json').chmod(0o600)
            env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(root/'fixture'),TERM='xterm-256color')
            terminal=Terminal(binary,root,env)
            def screen():return screen_text(bytes(terminal.output)).decode()
            def finish_frame():
                # A title can arrive before the rest of the same cursor-addressed draw.
                deadline=time.monotonic()+.25
                while time.monotonic()<deadline:drain(terminal.fd,terminal.output,.025)
            def clipboard():return [base64.b64decode(x).decode() for x in re.findall(rb'\x1b\]52;c;([A-Za-z0-9+/=]*)\x07',bytes(terminal.output))]
            def records():return [json.loads(line) for line in (root/'sessions/initial.jsonl').read_text().splitlines()]
            terminal.command('stream-case');terminal.expect(b'Reasoning')
            assert REASONING not in screen(),'reasoning was expanded by default'
            terminal.write(b'\x1br');terminal.expect(REASONING.encode());result['checks']['independent_folded_reasoning']=True
            phase1.set();terminal.expect(b'ROW_019');terminal.write(b'\x1b[5~');terminal.expect(b'Latest')
            finish_frame()
            before=next(line[:55] for line in screen().splitlines() if 'ROW_' in line)
            phase2.set();terminal.wait(lambda:'ANSWER_END' in str(records()));terminal.expect(b'in 123 out 45')
            finish_frame()
            after=next(line[:55] for line in screen().splitlines() if 'ROW_' in line)
            assert before==after,(before,after);result['checks']['live_scroll_anchor']=True
            latest_line=next((i,line) for i,line in enumerate(screen().splitlines()) if 'Latest' in line)
            row,line=latest_line;col=line.index('Latest')+2;terminal.write(f'\x1b[<0;{col};{row+1}M\x1b[<0;{col};{row+1}m'.encode());terminal.expect(b'ANSWER_END');result['checks']['mouse_follow_latest']=True
            assert 'ux-fixture' in screen() and 'last ux-served' in screen() and 'history ~' in screen(),screen();assert REASONING in str(records());result['checks']['model_usage_context_and_durable_reasoning']=True
            terminal.write(b'\x1bc');terminal.wait(lambda:len(clipboard())>=1);assert clipboard()[-1]==ANSWER;assert REASONING not in clipboard()[-1];result['checks']['answer_clipboard_exact']=True
            terminal.write(b'\x1bb');terminal.expect(b'Inspect / copy');terminal.write(b'\x1b[B\r');terminal.wait(lambda:len(clipboard())>=2);assert clipboard()[-1]=='fn copied_block() {}';result['checks']['code_block_clipboard_exact']=True
            terminal.shell("i=0; while [ $i -lt 50 ]; do printf 'TOOL_DETAIL_%03d\\n' \"$i\"; i=$((i+1)); done; exit 1")
            terminal.expect(b'TOOL_DETAIL_049')
            terminal.write(b'\x1bb');terminal.expect(b'Inspect / copy')
            # Streams now have separate bounded records. Navigate the actual
            # inspector instead of assuming the latest record is stdout.
            for _ in range(128):
                terminal.write(b'\x1b[6~'*8);finish_frame()
                if 'TOOL_DETAIL_049' in screen():break
                terminal.write(b'\x1b[B');finish_frame()
            else:raise AssertionError('captured tool output is absent from inspector')
            terminal.expect(b'TOOL_DETAIL_049');terminal.cancel_picker();result['checks']['long_tool_error_inspection']=True
            terminal.command('slow-case');terminal.expect(b'SLOW_');terminal.write(b'\x1bb');terminal.expect(b'Inspect / copy');terminal.write(b'\x03');terminal.expect(b'Interrupted');result['checks']['interrupt_from_inspector']=True
            terminal.folder(root/'other',mouse=False)
            checkpoint=root/'fixture/project-tabs.json'
            def active_cwd():
                v=json.loads(checkpoint.read_text());return v['metadata'][v['projects'][v['active']]]['cwd']
            terminal.wait(lambda:active_cwd()==str((root/'other').resolve()))
            terminal.command('error-case');terminal.expect(b'backend HTTP status: 500');terminal.write(b'\x1bb');terminal.expect(b'Inspect / copy');terminal.expect(b'backend HTTP status: 500');terminal.cancel_picker();result['checks']['provider_error_inspection']=True
            fcntl.ioctl(terminal.fd,termios.TIOCSWINSZ,struct.pack('HHHH',20,40,0,0));terminal.expect(b'[<]');terminal.expect(b'[>]')
            terminal.write(b'\x1b[<0;31;1M\x1b[<0;31;1m');terminal.wait(lambda:active_cwd()==str((root/'initial').resolve()));result['checks']['narrow_mouse_project_navigation']=True
            terminal.close();result['checks']['terminal_restored']=True
            result['request_count']=len(requests);assert len(requests)==3;result['passed']=True
    finally:
        phase1.set();phase2.set();server.shutdown()
        if terminal:
            args.evidence.parent.mkdir(parents=True,exist_ok=True);args.evidence.with_suffix('.pty.log').write_bytes(terminal.output)
            if terminal.pid:
                # Close the PTY before reaping a child; pending terminal output must not block teardown.
                os.close(terminal.fd)
                try:os.kill(terminal.pid,9);os.waitpid(terminal.pid,0)
                except (ProcessLookupError,ChildProcessError):pass
                terminal.pid=None
        args.evidence.parent.mkdir(parents=True,exist_ok=True);args.evidence.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()

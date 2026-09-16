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

def long_history_smoke(binary, evidence):
    """Seed 18k real journal rows, then read history while a new reply arrives."""
    import queue
    import subprocess
    requests, records = [], []
    started, release = threading.Event(), threading.Event()
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}}
    terminal = None
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
            number = len(requests)
            self.send_response(200); self.send_header('Content-Type', 'text/event-stream'); self.end_headers()
            def emit(kind, **payload):
                self.wfile.write(('event: '+kind+'\ndata: '+json.dumps({'type':kind, **payload})+'\n\n').encode()); self.wfile.flush()
            emit('response.created', response={'id':f'tail-{number}', 'model':'gpt-4.1'})
            if number <= 3:
                text = ''.join(f'P{number}_{row:04}\n' for row in range(6000)) + f'PAST_{number}_END'
            else:
                started.set()
                if not release.wait(20): return
                text = 'TAIL_LIVE_FINAL'
            emit('response.output_text.delta', delta=text)
            emit('response.completed', response={'id':f'tail-{number}', 'model':'gpt-4.1', 'status':'completed'})
    server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    evidence.parent.mkdir(parents=True,exist_ok=True)
    try:
        with TemporaryDirectory(prefix='transcript-tail-',dir=Path('.ops').resolve()) as raw:
            root=Path(raw)
            for name in ['initial','sessions','fixture']: (root/name).mkdir()
            config=root/'fixture'; journal=root/'sessions/initial.jsonl'
            (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'transcript-tail-fixture'})); (config/'auth.json').chmod(0o600)
            env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))}; env.update(ZENPI_HOME=str(config),TERM='xterm-256color')
            process=subprocess.Popen([str(binary),'--mode','headless','--session',str(journal)],cwd=root/'initial',env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            incoming=queue.Queue()
            def read():
                for line in process.stdout: records.append(line); incoming.put(json.loads(line))
                incoming.put(None)
            reader=threading.Thread(target=read,daemon=True); reader.start()
            try:
                for message in [{'id':str(i),'type':'prompt','text':f'seed history {i}'} for i in range(3)]+[{'id':'stop','type':'shutdown'}]:
                    process.stdin.write(json.dumps({'schema_version':2,**message})+'\n'); process.stdin.flush()
                    deadline=time.monotonic()+20
                    while True:
                        value=incoming.get(timeout=max(.01,deadline-time.monotonic()))
                        assert value is not None,'headless stopped before response'
                        if value.get('type')=='response' and value.get('id')==message['id']:
                            assert value['success'],value
                            break
                process.stdin.close(); assert process.wait(timeout=5)==0; reader.join(timeout=2)
            finally:
                if process.poll() is None: process.kill(); process.wait(timeout=5)
                result['headless_exit_code']=process.returncode
                result['headless_stderr']=process.stderr.read()
            assert len(requests)==3
            terminal=Terminal(binary,root,env)
            def screen(): return screen_text(bytes(terminal.output)).decode()
            def settle():
                deadline=time.monotonic()+.4
                while time.monotonic()<deadline: drain(terminal.fd,terminal.output,.02)
            terminal.command('tail live reply'); terminal.wait(started.is_set)
            terminal.write(b'\x1b[5~'); terminal.expect(b'Latest'); settle()
            before=next(line[:55] for line in screen().splitlines() if re.search(r'P[123]_\d',line))
            release.set(); terminal.wait(lambda:'TAIL_LIVE_FINAL' in journal.read_text()); settle()
            after=next(line[:55] for line in screen().splitlines() if re.search(r'P[123]_\d',line))
            result['reading_before']=before; result['reading_after']=after
            result['checks']['long_history_reading_anchor_stays_put']=before==after
            terminal.write(b'\x1b[F'); settle()
            result['latest_screen']=screen()
            result['checks']['latest_reply_visible_beyond_18000_rows']='TAIL_LIVE_FINAL' in screen()
            result['checks']['old_journal_and_new_reply_preserved']=all(marker in journal.read_text() for marker in ['PAST_1_END','PAST_2_END','PAST_3_END','TAIL_LIVE_FINAL'])
            result['checks']['exactly_four_provider_calls']=len(requests)==4
            terminal.close(preserve_draft=True); result['checks']['terminal_restored']=True
            evidence.with_suffix('.session.jsonl').write_bytes(journal.read_bytes())
    except Exception as error:
        result['error']=repr(error)
        raise
    finally:
        release.set(); server.shutdown(); server.server_close()
        if terminal:
            terminal.cleanup(); evidence.with_suffix('.pty.log').write_bytes(terminal.output)
        evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n')
        evidence.with_suffix('.jsonl.log').write_text(''.join(records))
        result['ok']='error' not in result and len(result['checks'])==5 and all(result['checks'].values())
        evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    assert result['ok'],result['checks']
    print(json.dumps({'ok':True,'checks':result['checks'],'http':len(requests)}))

def single_message_tail_smoke(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=False)
    results = [single_message_tail_case(binary, evidence / name, fenced) for name, fenced in (("prose", False), ("fenced", True))]
    summary = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "passed": all(result["passed"] for result in results), "cases": results}
    (evidence / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"passed": summary["passed"], "checks": [result["checks"] for result in results]}))
    if not summary["passed"]:
        raise SystemExit(1)


def single_message_tail_case(binary: Path, evidence: Path, fenced: bool) -> dict:
    gates = [threading.Event(), threading.Event()]
    requests, provider_errors = [], []
    opening = "```text\n" if fenced else ""
    initial = opening + "".join(f"ROW_{i:05}\n" for i in range(8180))
    appended = "".join(f"ROW_{i:05}\n" for i in range(8180, 9180))
    final = "SINGLE_STREAM_FINAL" + ("\n```" if fenced else "")
    expected = initial + appended + final
    assert len(expected.encode()) < 256 * 1024
    result = {"fenced": fenced, "checks": {}, "expected_sha256": hashlib.sha256(expected.encode()).hexdigest()}
    terminal = None

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()

            def emit(kind, **payload):
                body = json.dumps({"type": kind, **payload})
                self.wfile.write((f"event: {kind}\ndata: {body}\n\n").encode())
                self.wfile.flush()

            try:
                emit("response.created", response={"id": "single-tail", "model": "gpt-4.1"})
                emit("response.output_text.delta", delta=initial)
                if not gates[0].wait(30):
                    raise TimeoutError("reader did not release append gate")
                emit("response.output_text.delta", delta=appended)
                if not gates[1].wait(30):
                    raise TimeoutError("reader did not release finish gate")
                emit("response.output_text.delta", delta=final)
                emit("response.completed", response={"id": "single-tail", "model": "gpt-4.1", "status": "completed"})
            except Exception as error:
                provider_errors.append(repr(error))

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    evidence.mkdir(parents=True, exist_ok=False)
    try:
        with TemporaryDirectory(prefix="single-tail-", dir=Path(".ops").resolve()) as raw:
            root = Path(raw)
            for directory in ("initial", "sessions", "fixture"):
                (root / directory).mkdir()
            config = root / "fixture"
            (config / "config.toml").write_text(
                f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n'
            )
            (config / "auth.json").write_text(json.dumps({"OPENAI_API_KEY": "single-tail-fixture"}))
            (config / "auth.json").chmod(0o600)
            env = {key: value for key, value in os.environ.items() if not key.startswith(("ZENPI_", "OPENAI_"))}
            env.update(ZENPI_HOME=str(config), TERM="xterm-256color")
            assert env.get("HOME") == os.environ.get("HOME")
            assert env.get("CODEX_HOME") == os.environ.get("CODEX_HOME")
            terminal = Terminal(binary, root, env)
            checkpoint = config / "project-tabs.json"

            def screen():
                return screen_text(bytes(terminal.output)).decode()

            def settled():
                deadline = time.monotonic() + .4
                while time.monotonic() < deadline:
                    drain(terminal.fd, terminal.output, .02)
                return screen()

            def first_row(value):
                return next(line[:55] for line in value.splitlines() if re.search(r"ROW_\d{5}", line[:55]))

            terminal.command("single long reply")
            terminal.expect(b"ROW_08179")
            result["checks"]["initial_tail_visible_before_limit"] = "ROW_08179" in settled()
            terminal.write(b"\x1b[5~")
            terminal.expect(b"Latest")
            before = first_row(settled())
            gates[0].set()
            terminal.wait(lambda: "ROW_09179" in checkpoint.read_text())
            after = first_row(settled())
            result["reading_before"], result["reading_after"] = before, after
            result["checks"]["reading_anchor_survives_limit_crossing"] = before == after
            terminal.write(b"\x1b[F")
            result["after_end_screen"] = settled()
            result["checks"]["end_shows_tail_after_9180_rows"] = "ROW_09179" in result["after_end_screen"]
            gates[1].set()
            journal = root / "sessions/initial.jsonl"
            terminal.wait(lambda: "SINGLE_STREAM_FINAL" in journal.read_text())
            result["final_screen"] = settled()
            result["checks"]["following_shows_final_reply"] = "SINGLE_STREAM_FINAL" in result["final_screen"]
            turns = [json.loads(line) for line in journal.read_text().splitlines()]
            assistants = [row["turn"]["content"] for row in turns if row.get("kind") == "turn" and row["turn"]["role"] == "assistant"]
            result["checks"]["exact_single_assistant_turn_persisted"] = assistants == [expected]
            result["checks"]["exactly_one_streaming_request"] = len(requests) == 1 and requests[0].get("stream") is True
            terminal.close(preserve_draft=True)
            result["checks"]["terminal_restored"] = True
            (evidence / "session.jsonl").write_bytes(journal.read_bytes())
    except Exception as error:
        result["error"] = repr(error)
    finally:
        for gate in gates:
            gate.set()
        if terminal:
            terminal.cleanup()
            (evidence / "terminal.pty.log").write_bytes(terminal.output)
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        result["provider_errors"] = provider_errors
        result["passed"] = "error" not in result and not provider_errors and len(result["checks"]) == 7 and all(result["checks"].values())
        (evidence / "http.json").write_text(json.dumps(requests, ensure_ascii=False, indent=2) + "\n")
        (evidence / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    return result


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True,type=Path);parser.add_argument('--evidence',type=Path,default=Path('Docs/quality/stage1/ux125-pty-result.json'));parser.add_argument('--long-history-only',action='store_true');parser.add_argument('--single-message-only',action='store_true');args=parser.parse_args();binary=args.binary.resolve()
    if args.single_message_only:return single_message_tail_smoke(binary,args.evidence)
    if args.long_history_only:return long_history_smoke(binary,args.evidence)
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

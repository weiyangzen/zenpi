#!/usr/bin/env python3
"""Actual release PTY editor, literal paste, history, and restart evidence."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
import select
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from tempfile import TemporaryDirectory
from tui_project_workspace_smoke import Terminal as ProjectTerminal, screen_text


class Terminal(ProjectTerminal):
    def write(self, data):
        # Large input and a full redraw can otherwise block both ends of the
        # PTY in write(). Drain output while delivering every input byte.
        was_blocking=os.get_blocking(self.fd);os.set_blocking(self.fd,False)
        offset=0;deadline=time.monotonic()+12
        try:
            while offset<len(data):
                remaining=deadline-time.monotonic()
                if remaining<=0:raise TimeoutError(f'PTY input delivery timed out at {offset}/{len(data)} bytes')
                readable,writable,_=select.select([self.fd],[self.fd],[],min(.05,remaining))
                if readable:
                    try:self.output.extend(os.read(self.fd,65536))
                    except BlockingIOError:pass
                if writable:
                    try:offset+=os.write(self.fd,data[offset:offset+4096])
                    except BlockingIOError:pass
        finally:
            os.set_blocking(self.fd,was_blocking)


def burst_smoke(binary, evidence):
    """Unframed ordinary key bytes, actual terminal and local HTTP boundary."""
    import time
    requests, terminals = [], []
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}}
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            for kind, payload in [
                ('response.output_text.delta', {'delta': 'BURST_EXPLICIT_REPLY'}),
                ('response.completed', {'response': {'id': 'burst', 'model': 'gpt-4.1', 'status': 'completed'}}),
            ]:
                try:
                    self.wfile.write(('event: '+kind+'\ndata: '+json.dumps({'type': kind, **payload})+'\n\n').encode())
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError): return
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with TemporaryDirectory(prefix='ordinary-paste-pty-', dir=Path('.ops').resolve()) as raw:
            root = Path(raw); config = root/'fixture'
            for path in [root/'initial', root/'sessions', config]: path.mkdir(parents=True)
            (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'ordinary-paste-fixture'})); (config/'auth.json').chmod(0o600)
            env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
            env.update(ZENPI_HOME=str(config), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
            t = Terminal(binary, root, env); terminals.append(t)
            checkpoint = config/'project-tabs.json'
            def draft():
                value = json.loads(checkpoint.read_text()); active = value['projects'][value['active']]
                return next(row['draft']['input'] for row in value['project_state'] if row['name'] == active)
            def prompt_text():
                lines = screen_text(bytes(t.output)).decode().splitlines()
                index = next(i for i, line in enumerate(lines) if 'Prompt' in line)
                return '\n'.join(lines[index+1:])
            assert '~' not in prompt_text()
            start = time.monotonic(); t.write(b'~')
            t.wait(lambda: '~' in prompt_text())
            result['ascii_visible_ms'] = round((time.monotonic()-start)*1000, 2)
            assert result['ascii_visible_ms'] < 150
            t.wait(lambda: draft() == '~')
            assert not requests
            result['checks']['single_ascii_visible_below_150ms_and_idle_checkpointed'] = True
            t.write(b'\x15'); start = time.monotonic(); t.write('界'.encode())
            t.wait(lambda: '界' in prompt_text())
            result['unicode_visible_ms'] = round((time.monotonic()-start)*1000, 2)
            assert result['unicode_visible_ms'] < 150
            t.wait(lambda: draft() == '界')
            result['checks']['isolated_unicode_visible_below_150ms'] = True
            t.write(b'\x15AB')
            # Observe a real idle flush, then send the next Enter inside the
            # longer suppression window. CR must remain part of the paste.
            t.wait(lambda: 'AB' in prompt_text())
            t.write(b'\rCD')
            t.wait(lambda: draft() == 'AB\nCD')
            assert not requests
            result['checks']['enter_after_visible_idle_flush_stays_inside_burst'] = True
            t.clear_input()
            literal = 'PASTE_FIRST\n!touch must-not-run\n/exit\n世界e\u0301🧩\n'
            t.write(literal.replace('\n', '\r').encode())
            t.wait(lambda: draft() == literal)
            assert not requests
            assert not (root/'initial/must-not-run').exists()
            result['checks']['ordinary_multiline_no_submit_shell_or_exit_and_unicode_intact'] = True
            t.write(b'\x03'); t.cancel_picker()
            t.wait(lambda: draft() == literal)
            assert not requests
            result['checks']['cancel_keeps_entire_burst_draft_without_request'] = True
            t.close(preserve_draft=True)
            t = Terminal(binary, root, env); terminals.append(t)
            t.wait(lambda: draft() == literal)
            assert not requests
            result['checks']['independent_restart_restores_burst_without_resend'] = True
            t.write(b'\r'); t.expect(b'BURST_EXPLICIT_REPLY')
            assert len(requests) == 1
            latest = next(v for v in reversed(requests[0]['input']) if v.get('role') == 'user')
            assert literal.rstrip() in json.dumps(latest, ensure_ascii=False).replace('\\n', '\n'), latest
            result['checks']['deliberate_enter_after_quiet_submits_one_complete_message'] = True
            t.close(preserve_draft=True)
            result.update(status='passed', request_count=len(requests), tui_processes=len(terminals))
    except Exception as error:
        result.update(status='failed', error=str(error)); raise
    finally:
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.http.json').write_text(json.dumps(requests, ensure_ascii=False, indent=2)+'\n')
        evidence.write_text(json.dumps(result, indent=2)+'\n')
        for t in terminals: t.cleanup()
        server.shutdown(); server.server_close()
    return result


def large_paste_smoke(binary, evidence):
    """Real terminal, shared input owner, durable drafts and exact HTTP payloads."""
    import fcntl, struct, termios, time
    from tui_user_shell_smoke import drain
    requests, terminals = [], []
    gate_started, gate_release = threading.Event(), threading.Event()
    failure_started, failure_release = threading.Event(), threading.Event()
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}, 'payloads': []}
    def user_text(body):
        value = next(item for item in reversed(body['input']) if item.get('role') == 'user')['content']
        return value if isinstance(value, str) else ''.join(item.get('text', '') for item in value)
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body); number = len(requests); text = user_text(body)
            if text.startswith('FAIL_HOLD'):
                failure_started.set(); failure_release.wait(20)
            if text.startswith(('FAIL_LARGE', 'FAIL_HOLD')) or (text.startswith('FAIL_REPEAT') and sum(user_text(body)==text for body in requests)>1):
                self.send_response(503); self.send_header('Content-Type', 'application/json'); self.end_headers()
                self.wfile.write(b'{"error":{"message":"LARGE_PASTE_FAILURE"}}'); return
            self.send_response(200); self.send_header('Content-Type', 'text/event-stream'); self.end_headers()
            def emit(kind, **payload):
                self.wfile.write(('event: '+kind+'\ndata: '+json.dumps({'type':kind, **payload})+'\n\n').encode()); self.wfile.flush()
            try:
                emit('response.created', response={'id':f'large-{number}','model':'gpt-4.1'})
                if text == 'LARGE_INPUT_GATE':
                    gate_started.set()
                    while not gate_release.wait(.05): emit('response.output_text.delta', delta='')
                emit('response.output_text.delta', delta=f'LARGE_REPLY_{number}')
                emit('response.completed', response={'id':f'large-{number}','model':'gpt-4.1','status':'completed'})
            except (BrokenPipeError, ConnectionResetError): pass
    server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    config = None
    try:
        Path('.ops').mkdir(exist_ok=True)
        with TemporaryDirectory(prefix='large-paste-pty-',dir=Path('.ops').resolve()) as raw:
            root=Path(raw); config=root/'fixture'
            for path in [root/'initial',root/'sessions',config]: path.mkdir(parents=True)
            (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'large-paste-fixture'})); (config/'auth.json').chmod(0o600)
            env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))}; env.update(ZENPI_HOME=str(config),TERM='xterm-256color',NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
            t=Terminal(binary,root,env); terminals.append(t); checkpoint=config/'project-tabs.json'; journal=root/'sessions/initial.jsonl'
            def saved(): return json.loads(checkpoint.read_text())
            def draft():
                value=saved(); return next(row['draft'] for row in value['project_state'] if row['name']==value['projects'][value['active']])
            def shown():
                lines=screen_text(bytes(t.output)).decode().splitlines()
                index=next(i for i,line in enumerate(lines) if 'Prompt' in line)
                return '\n'.join(lines[index+1:])
            def paste(text): t.write(b'\x1b[200~'+text.encode()+b'\x1b[201~')
            def wait_draft(text): t.wait(lambda:draft()['input']==text)
            def clear(): t.clear_input()
            def abandon_unknown():
                def events():
                    return [record['event'] for record in (json.loads(line) for line in journal.read_text().splitlines()) if record.get('kind')=='event']
                decided={event['operation_id'] for event in events() if event.get('type')=='operation_recovery_decided'}
                pending=[event['operation_id'] for event in events() if event.get('type')=='operation_finished' and event.get('outcome')=='unknown_outcome' and event['operation_id'] not in decided]
                assert len(pending)==1, pending
                operation_id=pending[0]; t.command('/recovery abandon '+operation_id+' --yes')
                t.wait(lambda:any(event.get('type')=='operation_recovery_decided' and event.get('operation_id')==operation_id for event in events()))
                wait_draft('')
            def send_exact(text):
                previous=len(requests); t.deliberate_enter(); t.wait(lambda:len(requests)==previous+1)
                t.expect(f'LARGE_REPLY_{previous+1}'.encode()); t.wait(lambda:' Ready |' in screen_text(bytes(t.output)).decode().splitlines()[1])
                actual=user_text(requests[-1]); assert actual==text,(len(actual),len(text),repr(actual[:80]),repr(actual[-80:]))
                result['payloads'].append({'bytes':len(text.encode()),'sha256':hashlib.sha256(text.encode()).hexdigest(),'request':len(requests)})
                wait_draft('')
            for text, count in [('a'*1000,0),(' '+('b'*1001)+'\n',1),('\r\n'+('界'*1001)+'\r',1)]:
                expected=text.replace('\r\n','\n').replace('\r','\n'); before=len(requests); paste(text); wait_draft(expected)
                assert len(draft().get('paste_folds',[]))==count and len(requests)==before
                if count: t.expect(b'Pasted Content')
                send_exact(expected)
            result['checks']['1000_1001_unicode_crlf_threshold_and_exact_http']=True
            first='A'*1001; paste(first); wait_draft(first); first_id=draft()['paste_folds'][0]['id']
            literal=f' [Pasted Content 1001 chars] #{first_id} '
            paste(literal); paste('B'*1001); wait_draft(first+literal+'B'*1001); removed_id=draft()['paste_folds'][1]['id']
            t.write(b'\x7f'); wait_draft(first+literal); paste('C'*1001); wait_draft(first+literal+'C'*1001)
            assert draft()['paste_folds'][1]['id']>removed_id
            t.write(b'\x1b\r'); t.wait(lambda:len(draft()['paste_folds'])==1 and draft()['cursor']==len((first+literal).encode()))
            t.write(b'\x1b[3~'); paste('Z'); expected=first+literal+'Z'+'C'*1000; wait_draft(expected)
            send_exact(expected)
            result['checks']['same_size_ids_literal_label_atomic_delete_expand_edit_exact_payload']=True
            paste('pre\n'); folded='HIDDEN_LINE\n'*100; paste(folded); paste('tail'); expected='pre\n'+folded+'tail'; wait_draft(expected)
            for columns in (32,1,140):
                fcntl.ioctl(t.fd,termios.TIOCSWINSZ,struct.pack('HHHH',40,columns,0,0)); drain(t.fd,t.output,.08)
                assert draft()['input']==expected
            t.expect(b'Pasted Content'); assert 'HIDDEN_LINE' not in shown()
            t.write(b'\x01'); t.wait(lambda:draft()['cursor']==4)
            t.write(b'\x1b[C'); t.wait(lambda:draft()['cursor']==len(('pre\n'+folded).encode()))
            t.write(b'\x1b[D\x0b'); wait_draft('pre\n'); clear()
            paste('界'*1001); paste('👨\u200d👩\u200d👧\u200d👦'); t.write(b'\x7f'); wait_draft('界'*1001)
            assert len(draft()['paste_folds'])==1
            t.write(b'\x17'); wait_draft('')
            result['checks']['real_arrows_line_word_delete_grapheme_and_narrow_resize']=True
            ordinary='ordinary '+('q'*1001)
            t.write(ordinary.encode()); t.wait(lambda:'Pasted Content' in shown()); t.write(b'\r!touch must-not-run\r/exit\rTAIL')
            expected=ordinary+'\n!touch must-not-run\n/exit\nTAIL'; wait_draft(expected)
            assert draft()['paste_folds'] and not (root/'initial/must-not-run').exists() and len(requests)==4
            send_exact(expected)
            result['checks']['ordinary_burst_fold_idle_flush_cr_window_and_no_auto_execution']=True
            payload='D'*1001; paste(payload); wait_draft(payload); t.write(b'\x1b[D'); t.wait(lambda:draft()['cursor']==0); original=draft()
            t.write(b'\x12ordinary'); t.expect(b'History search: ordinary'); t.cancel_picker(); wait_draft(payload)
            assert draft()['paste_folds']==original['paste_folds'] and draft()['cursor']==0
            other=root/'other'; other.mkdir(); t.folder(other,mouse=False); t.wait(lambda:saved()['active']==1)
            paste('E'*1001); wait_draft('E'*1001); t.folder(root/'initial',mouse=False); t.wait(lambda:saved()['active']==0); wait_draft(payload)
            assert draft()['paste_folds']==original['paste_folds']; before=len(requests)
            t.close(preserve_draft=True); t=Terminal(binary,root,env); terminals.append(t); wait_draft(payload)
            assert draft()['paste_folds']==original['paste_folds'] and draft()['cursor']==0 and len(requests)==before
            send_exact(payload)
            result['checks']['history_escape_project_isolation_and_independent_restart_full_fold']=True
            t.command('LARGE_INPUT_GATE'); t.wait(gate_started.is_set); gate_count=len(requests)
            def input_events():
                return [record['event']['change'] for record in (json.loads(line) for line in journal.read_text().splitlines()) if record.get('kind')=='event' and record['event'].get('type')=='input_queue']
            def changes(action,input_id=None):
                return [row for row in input_events() if row['action']==action and (input_id is None or row.get('id',row.get('input',{}).get('id'))==input_id)]
            queued='QUEUED '+('Q'*1001)+'\n'; paste(queued); wait_draft(queued); t.deliberate_enter()
            t.wait(lambda:any(row.get('input',{}).get('text')==queued for row in changes('enqueued')))
            auto_id=next(row['input']['id'] for row in changes('enqueued') if row['input']['text']==queued)
            t.expect(b'Input owner confirmed'); wait_draft('')
            edited='EDITED '+('e'*1001); paste(f'/input edit {auto_id} 0 '); paste(edited); wait_draft(f'/input edit {auto_id} 0 '+edited); t.deliberate_enter()
            t.wait(lambda:changes('edited',auto_id)); assert changes('edited',auto_id)[-1]['text']==edited; wait_draft('')
            t.command('/input follow-up remove-large '+('R'*1001)); t.wait(lambda:changes('enqueued','remove-large')); t.command('/input cancel remove-large'); t.wait(lambda:changes('cancelled','remove-large'))
            # The scheduled queue has its existing separate16-item/256KiB budget.
            # Canonical payload length, never compact labels, determines admission.
            (root/'initial/queued.txt').write_text('queued attachment fixture')
            first_shell='scheduled kept '+('s'*150000)+' @queued.txt'; paste(first_shell); wait_draft(first_shell); t.deliberate_enter()
            t.wait(lambda:len(saved()['scheduled_inputs'])==1); job=saved()['scheduled_inputs'][0]; wait_draft('')
            second_shell='scheduled rejected '+('t'*150000)+' @queued.txt'; paste(second_shell); wait_draft(second_shell); folds=draft()['paste_folds']; t.deliberate_enter()
            t.expect(b'Scheduled queue byte limit exceeded'); t.wait(lambda:draft()['input']==second_shell and len(saved()['scheduled_inputs'])==1)
            assert draft()['paste_folds']==folds and len(requests)==gate_count
            clear(); t.command('/scheduled cancel '+job['id']); t.wait(lambda:not saved()['scheduled_inputs'])
            gate_release.set(); t.expect(f'LARGE_REPLY_{gate_count+1}'.encode()); t.wait(lambda:len(requests)==gate_count+1 and ' Ready |' in screen_text(bytes(t.output)).decode().splitlines()[1])
            assert any(item.get('content')==edited for item in requests[-1]['input'] if item.get('role')=='user')
            assert not any('R'*1001 in json.dumps(body) for body in requests[gate_count:])
            assert sum(auto_id in row['ids'] for row in changes('applied'))==1
            result['input_events']=input_events()
            result['checks']['busy_full_input_owner_edit_cancel_apply_exactly_once']=True
            result['checks']['folded_scheduled_payload_cannot_bypass_256k_and_rejection_restores_fold']=True
            invalid='/missing '+('x'*1001); paste(invalid); wait_draft(invalid); original=draft(); before=len(requests); t.deliberate_enter()
            t.expect(b'Command rejected'); wait_draft(invalid); assert draft()['paste_folds']==original['paste_folds'] and len(requests)==before; clear()
            result['checks']['synchronous_rejection_restores_exact_fold_metadata']=True
            failure='FAIL_LARGE '+('f'*1001); paste(failure); wait_draft(failure); original=draft(); before=len(requests); t.deliberate_enter()
            t.wait(lambda:len(requests)==before+1); t.expect(b'Request failed'); wait_draft(failure); assert draft()['paste_folds']==original['paste_folds']; clear(); abandon_unknown()
            paste('FAIL_HOLD '+('h'*1001)); wait_draft('FAIL_HOLD '+('h'*1001)); t.deliberate_enter(); t.wait(failure_started.is_set)
            paste('newer live draft'); wait_draft('newer live draft'); errors_before=sum(message['role']=='Error' for row in saved()['project_state'] for message in row['messages']); failure_release.set(); t.wait(lambda:sum(message['role']=='Error' for row in saved()['project_state'] for message in row['messages'])>errors_before); t.expect(b'Request failed'); wait_draft('newer live draft'); clear(); abandon_unknown()
            result['checks']['provider_failure_recovers_fold_and_preserves_newer_draft']=True
            repeated='FAIL_REPEAT '+('r'*1001); paste(repeated); wait_draft(repeated); prior_id=draft()['paste_folds'][0]['id']; send_exact(repeated)
            paste(repeated); wait_draft(repeated); t.write(b'\x1b[D'); t.wait(lambda:draft()['cursor']==0); original=draft(); assert original['paste_folds'][0]['id']>prior_id
            before=len(requests); errors_before=sum(message['role']=='Error' for row in saved()['project_state'] for message in row['messages']); t.deliberate_enter()
            t.wait(lambda:len(requests)==before+1 and sum(message['role']=='Error' for row in saved()['project_state'] for message in row['messages'])>errors_before); wait_draft(repeated)
            assert draft()['paste_folds']==original['paste_folds'] and draft()['cursor']==0; clear(); abandon_unknown()
            result['checks']['same_payload_success_then_failure_restores_only_current_id_and_cursor']=True
            full='M'*(256*1024); paste(full); wait_draft(full); before=draft(); count=len(requests); paste('x'); t.expect(b'Input exceeds'); assert draft()==before and len(requests)==count; clear()
            result['checks']['canonical_256k_limit_rejects_extra_byte_despite_small_label']=True
            config.chmod(0o500); saved_bytes=checkpoint.read_bytes(); durable='界'*1001
            try:
                paste(durable); t.expect(b'Draft not saved'); t.expect(b'Pasted Content'); assert checkpoint.read_bytes()==saved_bytes and len(requests)==count
            finally: config.chmod(0o700)
            wait_draft(durable); t.wait(lambda:'Draft not saved' not in shown()); assert len(draft()['paste_folds'])==1
            result['checks']['permission_failure_visible_preserves_old_file_and_live_payload_then_retries']=True
            t.close(preserve_draft=True); value=saved(); active=value['projects'][value['active']]
            row=next(row for row in value['project_state'] if row['name']==active); row['draft']['paste_folds'][0]['start_byte']=1
            checkpoint.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
            t=Terminal(binary,root,env); terminals.append(t); t.expect(b'Invalid paste metadata'); wait_draft(durable)
            assert not draft()['paste_folds'] and len(requests)==count; send_exact(durable)
            result['checks']['invalid_utf8_fold_range_restart_safely_unfolds_full_text']=True
            # Restore a valid bounded checkpoint close to4MiB and exercise the
            # real next-paste admission boundary in a new process.
            clear(); t.close(preserve_draft=True); value=saved()
            for row in value['project_state']:
                row['messages']=[]; row['draft'].update(input='',cursor=0,paste_folds=[],history=[])
            row=next(row for row in value['project_state'] if row['name']==value['projects'][value['active']])
            row['messages']=[{'role':'System','text':'m'*(256*1024),'blocks':[]} for _ in range(15)]
            row['draft']['input']='keep'; row['draft']['cursor']=4
            encoded=json.dumps(value,ensure_ascii=False,indent=2).encode(); assert len(encoded)<4*1024*1024
            checkpoint.write_bytes(encoded); t=Terminal(binary,root,env); terminals.append(t); wait_draft('keep'); count=len(requests)
            paste('x'*(256*1024-4)); t.expect(b'4 MiB project checkpoint budget'); assert draft()['input']=='keep' and len(requests)==count
            result['checks']['actual_pretty_checkpoint_4m_limit_rejects_large_paste_atomically']=True
            t.close(preserve_draft=True)
            result.update(passed=True,request_count=len(requests),tui_processes=len(terminals))
    except Exception as error:
        result.update(passed=False,error=str(error)); raise
    finally:
        gate_release.set(); failure_release.set()
        if config is not None and config.exists(): config.chmod(0o700)
        for terminal in terminals: terminal.cleanup()
        server.shutdown(); server.server_close(); evidence.parent.mkdir(parents=True,exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(terminal.output) for terminal in terminals))
        evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n')
        evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    return result


def queued_shell_paste_smoke(binary, evidence):
    """Accepted shell FIFO, real provider gates, approval and child processes."""
    requests, terminals, gates = [], [], {}
    result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':{},'accepted':[]}
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args): pass
        def do_POST(self):
            body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body);number=len(requests)
            text=next(item['content'] for item in reversed(body['input']) if item.get('role')=='user')
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
            def emit(kind,**value):self.wfile.write(('data: '+json.dumps(dict(type=kind,**value))+'\n\n').encode());self.wfile.flush()
            try:
                emit('response.created',response={'id':str(number),'model':'gpt-4.1'})
                if text in gates:
                    started,release=gates[text];started.set()
                    while not release.wait(.05):emit('response.output_text.delta',delta='')
                emit('response.output_text.delta',delta=f'SHELL_GATE_DONE_{number}')
                emit('response.completed',response={'id':str(number),'model':'gpt-4.1','status':'completed'})
            except (BrokenPipeError,ConnectionResetError): pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        with TemporaryDirectory(prefix='queued-shell-paste-',dir=Path('.ops').resolve()) as raw:
            root=Path(raw);config=root/'fixture'
            for path in [root/'initial',root/'sessions',config]:path.mkdir(parents=True)
            (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'queued-shell-fixture'}));(config/'auth.json').chmod(0o600)
            env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(config),TERM='xterm-256color',NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
            t=Terminal(binary,root,env);terminals.append(t);checkpoint=config/'project-tabs.json';journal=root/'sessions/initial.jsonl'
            def saved():return json.loads(checkpoint.read_text())
            def draft():
                value=saved();return next(row['draft'] for row in value['project_state'] if row['name']==value['projects'][value['active']])
            def wait_draft(text):t.wait(lambda:draft()['input']==text)
            def paste(text):t.write(b'\x1b[200~'+text.encode()+b'\x1b[201~')
            def clear():t.clear_input()
            def start_gate():
                marker=f'SHELL_QUEUE_GATE_{len(gates)}';pair=(threading.Event(),threading.Event());gates[marker]=pair;t.command(marker);t.wait(pair[0].is_set);return pair[1]
            def enqueue(original,cursor_at_start=True):
                count=len(saved()['scheduled_inputs']);paste(original);wait_draft(original)
                if cursor_at_start:t.write(b'\x1b[D');t.wait(lambda:draft()['cursor']==0)
                snapshot=draft();t.deliberate_enter()
                t.wait(lambda:len(saved()['scheduled_inputs'])==count+1);wait_draft('');entry=saved()['scheduled_inputs'][-1]
                result['accepted'].append({'entry':entry,'draft_before':snapshot});return entry,snapshot
            def finish(release,allow):
                release.set();t.wait(lambda:'Approval required' in screen_text(bytes(t.output)).decode().splitlines()[1])
                t.write(b'y\r' if allow else b'n\r')
                if allow:t.wait(lambda:'Local shell exit 0' in screen_text(bytes(t.output)).decode().splitlines()[1])
                else:t.wait(lambda:'Request failed' in screen_text(bytes(t.output)).decode().splitlines()[1])
                t.wait(lambda:not saved()['scheduled_inputs'])
                result['journal_records']=[json.loads(line) for line in journal.read_text().splitlines()]
                result['actual_files']={path.name:{'text':path.read_text(),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()} for path in (root/'initial').iterdir() if path.is_file()}
            def rejected_exact(original,snapshot):
                t.wait(lambda:bool(draft()['input']));actual=draft()
                assert actual['input']==original,(repr(actual['input'][:35]),repr(original[:35]),repr(actual['input'][-8:]))
                assert actual['paste_folds']==snapshot['paste_folds'] and actual['cursor']==0
                clear()
            original='  !printf executed > accepted.txt # '+('x'*1001)+'\n'
            release=start_gate();entry,first=enqueue(original);assert not (root/'initial/accepted.txt').exists();finish(release,True)
            assert (root/'initial/accepted.txt').read_text()=='executed';wait_draft('');result['checks']['accepted_folded_shell_executes_after_actual_approval']=True
            release=start_gate();entry,second=enqueue(original);assert second['paste_folds'][0]['id']>first['paste_folds'][0]['id'];finish(release,False);rejected_exact(original,second)
            assert entry['text']==original;result['checks']['success_retired_metadata_then_same_shell_failure_restores_new_id_cursor_original_bytes']=True
            cancelled=' \t!touch cancelled-must-not-exist # '+('c'*1001)+'\n'
            # Paste sanitization turns tab into one space, as in all composer paths.
            cancelled=cancelled.replace('\t',' ')
            release=start_gate();entry,first=enqueue(cancelled);t.command('/scheduled cancel '+entry['id']);t.wait(lambda:not saved()['scheduled_inputs']);wait_draft('')
            entry,second=enqueue(cancelled);assert second['paste_folds'][0]['id']>first['paste_folds'][0]['id'];finish(release,False);rejected_exact(cancelled,second)
            assert not (root/'initial/cancelled-must-not-exist').exists();result['checks']['accepted_cancel_retires_old_fold_before_identical_resubmission_failure']=True
            original_edit='  !touch edited-original-must-not-exist # '+('e'*1001)+'\n';edited='  !printf edited > accepted-edited.txt # '+('z'*1001)+'\n'
            release=start_gate();entry,first=enqueue(original_edit);command=f"/scheduled edit {entry['id']} 0 {edited}";paste(command);wait_draft(command);t.deliberate_enter()
            t.wait(lambda:saved()['scheduled_inputs'][0]['revision']==1);assert saved()['scheduled_inputs'][0]['text']==edited;wait_draft('');finish(release,True)
            assert (root/'initial/accepted-edited.txt').read_text()=='edited' and not (root/'initial/edited-original-must-not-exist').exists()
            release=start_gate();entry,second=enqueue(original_edit);assert second['paste_folds'][0]['id']>first['paste_folds'][0]['id'];finish(release,False);rejected_exact(original_edit,second)
            result['checks']['accepted_edit_executes_exact_replacement_and_retires_original_fold']=True
            duplicate='  !touch duplicate-cancel-must-not-exist # '+('d'*1001)+'\n'
            release=start_gate();first_entry,first=enqueue(duplicate);second_entry,second=enqueue(duplicate,False)
            assert first_entry['id']!=second_entry['id'] and first['paste_folds']!=second['paste_folds'] and first['cursor']!=second['cursor']
            t.command('/scheduled cancel '+second_entry['id']);t.wait(lambda:len(saved()['scheduled_inputs'])==1);assert saved()['scheduled_inputs'][0]['id']==first_entry['id'];wait_draft('')
            result['identity_cancel']={'cancelled_id':second_entry['id'],'remaining':saved()['scheduled_inputs'],'expected_first_draft':first}
            finish(release,False);rejected_exact(duplicate,first);assert not (root/'initial/duplicate-cancel-must-not-exist').exists()
            result['checks']['two_identical_pending_shells_cancel_second_by_id_restores_first_metadata']=True
            duplicate='  !touch duplicate-edit-must-not-exist # '+('f'*1001)+'\n'
            release=start_gate();first_entry,first=enqueue(duplicate);second_entry,second=enqueue(duplicate,False)
            t.command('/scheduled edit '+second_entry['id']+' 0 printf identity-edited > duplicate-edited.txt');t.wait(lambda:saved()['scheduled_inputs'][1]['revision']==1);wait_draft('')
            result['identity_edit']={'edited_id':second_entry['id'],'remaining':saved()['scheduled_inputs'],'expected_first_draft':first}
            release.set();t.wait(lambda:'Approval required' in screen_text(bytes(t.output)).decode().splitlines()[1]);t.write(b'n\r');t.wait(lambda:bool(draft()['input']))
            actual=draft();assert actual['input']==duplicate and actual['paste_folds']==first['paste_folds'] and actual['cursor']==first['cursor']
            t.wait(lambda:'Approval required' in screen_text(bytes(t.output)).decode().splitlines()[1]);t.write(b'y\r');t.wait(lambda:(root/'initial/duplicate-edited.txt').exists());t.wait(lambda:not saved()['scheduled_inputs'])
            assert (root/'initial/duplicate-edited.txt').read_text()=='identity-edited' and not (root/'initial/duplicate-edit-must-not-exist').exists();clear()
            result['journal_records']=[json.loads(line) for line in journal.read_text().splitlines()]
            result['actual_files']={path.name:{'text':path.read_text(),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()} for path in (root/'initial').iterdir() if path.is_file()}
            result['checks']['two_identical_pending_shells_edit_second_preserves_first_metadata_and_executes_second']=True
            restart='  !touch restart-must-not-exist # '+('r'*1001)+'\n';release=start_gate();entry,_=enqueue(restart);t.close(preserve_draft=True);release.set();count=len(requests)
            t=Terminal(binary,root,env);terminals.append(t);t.wait(lambda:any(row['id']==entry['id'] and row['interrupted'] for row in saved()['scheduled_inputs']))
            assert saved()['scheduled_inputs'][0]['text']==restart and not (root/'initial/restart-must-not-exist').exists() and len(requests)==count
            t.command('/scheduled cancel '+entry['id']);t.wait(lambda:not saved()['scheduled_inputs']);t.close(preserve_draft=True)
            result['checks']['independent_restart_retains_full_accepted_shell_without_auto_execution']=True
            result.update(passed=True,request_count=len(requests),tui_processes=len(terminals))
    except Exception as error:
        result.update(passed=False,error=str(error));raise
    finally:
        for _,release in gates.values():release.set()
        for terminal in terminals:terminal.cleanup()
        server.shutdown();server.server_close();evidence.parent.mkdir(parents=True,exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals));evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n');evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    return result


def kill_yank_smoke(binary, evidence):
    """Real modified keys, canonical drafts, owner receipts and exact HTTP bytes."""
    import fcntl, struct, termios
    from tui_user_shell_smoke import drain
    requests=[]; terminals=[]; gates={name:(threading.Event(),threading.Event()) for name in ('YANK_GATE','YANK_FAIL')}
    result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':{},'snapshots':[]}
    def user_text(body):
        content=next(item['content'] for item in reversed(body['input']) if item.get('role')=='user')
        return content if isinstance(content,str) else ''.join(item.get('text','') for item in content)
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_POST(self):
            body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body);number=len(requests);text=user_text(body)
            if text=='YANK_FAIL':
                gates[text][0].set();gates[text][1].wait(30)
                self.send_response(503);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(b'{"error":{"message":"YANK_EXPECTED_FAILURE"}}');return
            self.send_response(200);self.send_header('Content-Type','text/event-stream');self.end_headers()
            def emit(kind,**value):self.wfile.write(('data: '+json.dumps(dict(type=kind,**value))+'\n\n').encode());self.wfile.flush()
            try:
                emit('response.created',response={'id':f'yank-{number}','model':'gpt-4.1'})
                if text=='YANK_GATE':
                    gates[text][0].set()
                    while not gates[text][1].wait(.05):emit('response.output_text.delta',delta='')
                emit('response.output_text.delta',delta=f'YANK_REPLY_{number}')
                emit('response.completed',response={'id':f'yank-{number}','model':'gpt-4.1','status':'completed'})
            except (BrokenPipeError,ConnectionResetError):pass
    server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        with TemporaryDirectory(prefix='kill-yank-pty-',dir=Path('.ops').resolve()) as raw:
            root=Path(raw);config=root/'fixture'
            for d in [root/'initial',root/'sessions',config]:d.mkdir(parents=True)
            (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'kill-yank-fixture-fake'}));(config/'auth.json').chmod(0o600)
            env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(config),TERM='xterm-256color',NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
            t=Terminal(binary,root,env);terminals.append(t);checkpoint=config/'project-tabs.json';journal=root/'sessions/initial.jsonl';expected_http=0
            def saved():return json.loads(checkpoint.read_text())
            def draft():
                value=saved();return next(row['draft'] for row in value['project_state'] if row['name']==value['projects'][value['active']])
            def wait_text(text):t.wait(lambda:draft()['input']==text)
            def paste(text):t.write(b'\x1b[200~'+text.encode()+b'\x1b[201~')
            def check(name,text):
                wait_text(text);drain(t.fd,t.output,.08);assert len(requests)==expected_http
                assert b'\x1b]52;' not in t.output
                result['checks'][name]=True;result['snapshots'].append({'check':name,'draft':draft(),'http_count':len(requests),'screen':screen_text(bytes(t.output)).decode()})
            def send_exact(text):
                nonlocal expected_http
                t.deliberate_enter();expected_http+=1;t.wait(lambda:len(requests)==expected_http)
                t.expect(f'YANK_REPLY_{expected_http}'.encode());t.wait(lambda:' Ready |' in screen_text(bytes(t.output)).decode().splitlines()[1]);wait_text('')
                assert user_text(requests[-1])==text
                result.setdefault('sent_payloads',[]).append({'text':text,'bytes':len(text.encode()),'sha256':hashlib.sha256(text.encode()).hexdigest()})
            paste('first\nbeta');wait_text('first\nbeta');t.write(b'\x01\x1b[C\x15');check('logical_line_ctrl_u_keeps_previous_line','first\neta')
            t.write(b'\x19');check('ctrl_y_restores_at_cursor','first\nbeta');t.write(b'\x01\x15');check('bol_ctrl_u_removes_only_previous_lf','firstbeta')
            t.write(b'\x19\x1b[1;5H\x15\x05\x0b');check('first_bol_noop_and_eol_ctrl_k_lf','firstbeta');t.write(b'\x19');wait_text('first\nbeta');t.clear_input()
            paste('head\n');wait_text('head\n');hidden='hidden\n'*200;paste(hidden);wait_text('head\n'+hidden);t.write(b'\x15');check('ctrl_u_fold_projection_ignores_hidden_newlines','head\n');t.write(b'\x19');check('hidden_multiline_payload_yanked_as_plain_text','head\n'+hidden);assert not draft()['paste_folds'];t.clear_input()
            paste('alpha beta');wait_text('alpha beta');t.write(b'\x17\x19\x19');check('single_buffer_repeat_yank','alpha betabeta');t.clear_input()
            paste('word');wait_text('word');t.write(b'\x15\x15');paste('x');wait_text('x');t.write(b'\x7f\x1b[3~\x19');check('empty_kill_and_plain_deletes_preserve_last_kill','word');t.clear_input()
            paste('left right');wait_text('left right');t.write(b'\x1b[1;5H\x1b[3;5~\x19');check('forward_word_kill_and_yank','left right');t.clear_input()
            literal='[Pasted Content 1001 chars] #1 ';payload='界e\u0301👨\u200d👩\u200d👧\u200d👦'*150
            paste(literal);wait_text(literal);paste(payload);wait_text(literal+payload);old_id=draft()['paste_folds'][0]['id'];t.write(b'\x17');wait_text(literal)
            paste('z'*1001);wait_text(literal+'z'*1001);t.write(b'\x1b[D\x19');check('fold_canonical_unicode_yank_plain_next_fold_shifted',literal+payload+'z'*1001)
            assert len(draft()['paste_folds'])==1 and draft()['paste_folds'][0]['id']>old_id and draft()['paste_folds'][0]['start_byte']==len((literal+payload).encode())
            before=draft();fcntl.ioctl(t.fd,termios.TIOCSWINSZ,struct.pack('HHHH',1,1,0,0));drain(t.fd,t.output,.1);fcntl.ioctl(t.fd,termios.TIOCSWINSZ,struct.pack('HHHH',40,140,0,0));check('one_column_resize_preserves_yanked_payload',before['input']);assert draft()==before
            t.write(b'\x15');wait_text('z'*1001);t.write(b'\x19');check('ctrl_u_uses_logical_line_after_soft_wrap',before['input']);assert draft()==before
            t.write(b'\x17\x19');wait_text(before['input']);send_exact(before['input']);t.write(b'\x19');check('successful_submit_preserves_kill',payload);t.clear_input()
            paste('slash-survivor');wait_text('slash-survivor');t.write(b'\x15');wait_text('');t.command('/status');wait_text('');t.write(b'\x19');check('local_slash_preserves_kill','slash-survivor')
            t.write(b'\x12\x19');t.cancel_picker();wait_text('slash-survivor');t.write(b'\x19');check('history_search_focus_and_escape_preserve_kill','slash-survivorslash-survivor');t.clear_input()
            paste('A-kill');wait_text('A-kill');t.write(b'\x15');wait_text('');other=root/'other';other.mkdir();t.folder(other,mouse=False);t.wait(lambda:saved()['active']==1);t.write(b'\x19');check('new_project_empty_kill_buffer','')
            paste('B-kill');wait_text('B-kill');t.write(b'\x15\x19');check('project_b_own_kill','B-kill');t.folder(root/'initial',mouse=False);t.wait(lambda:saved()['active']==0);t.write(b'\x19');check('project_a_retains_independent_kill','A-kill')
            t.write(b'\x14');t.expect(b'Open project folder');t.write(b'\x19');t.cancel_picker();check('folder_picker_ctrl_y_does_not_edit_composer','A-kill');t.clear_input()
            paste('held-kill');wait_text('held-kill');t.write(b'\x15');wait_text('');t.write(b'A\x19');check('ordinary_held_ascii_flush_before_yank','Aheld-kill');send_exact('Aheld-kill')
            t.command('YANK_GATE');expected_http+=1;t.wait(gates['YANK_GATE'][0].is_set);wait_text('')
            def records():
                rows=[json.loads(line) for line in journal.read_text().splitlines()];result['journal_records']=rows;return rows
            def changes(action):return [r['event']['change'] for r in records() if r.get('kind')=='event' and r['event'].get('type')=='input_queue' and r['event']['change']['action']==action]
            t.command('queued original');t.wait(lambda:changes('enqueued'));wait_text('');input_id=changes('enqueued')[-1]['input']['id']
            t.command(f'/input edit {input_id} 0 queued edited');t.wait(lambda:changes('edited'));wait_text('');assert changes('edited')[-1]['text']=='queued edited'
            t.command(f'/input cancel {input_id}');t.wait(lambda:changes('cancelled'));wait_text('');t.write(b'\x19');check('busy_input_owner_edit_cancel_do_not_change_kill','held-kill')
            gates['YANK_GATE'][1].set();t.expect(f'YANK_REPLY_{expected_http}'.encode());t.wait(lambda:' Ready |' in screen_text(bytes(t.output)).decode().splitlines()[1]);check('background_success_keeps_current_yanked_draft','held-kill');result['input_events']=[r['event'] for r in records() if r.get('event',{}).get('type')=='input_queue'];t.clear_input()
            t.command('YANK_FAIL');expected_http+=1;t.wait(gates['YANK_FAIL'][0].is_set);wait_text('');paste('new-kill');wait_text('new-kill');t.write(b'\x15');wait_text('');paste('new-draft');wait_text('new-draft')
            gates['YANK_FAIL'][1].set();t.expect(b'Request failed');wait_text('new-draft');t.write(b'\x19');check('async_failure_does_not_roll_back_newer_kill_or_draft','new-draftnew-kill');t.clear_input()
            unknown=[r['event']['operation_id'] for r in records() if r.get('event',{}).get('type')=='operation_finished' and r['event'].get('outcome')=='unknown_outcome']
            assert unknown;t.command('/recovery abandon '+unknown[-1]+' --yes');wait_text('')
            # Approval is a real local subprocess boundary, denied before side effect.
            t.command('!touch yank-approval-must-not-run');t.expect(b'Enter confirm');t.write(b'\x19');drain(t.fd,t.output,.1);assert not (root/'initial/yank-approval-must-not-run').exists() and draft()['input']=='';t.write(b'n\r');t.expect(b'Request failed');wait_text('!touch yank-approval-must-not-run');t.clear_input();result['checks']['approval_ctrl_y_neither_yanks_nor_confirms']=True
            paste('persisted-yank');wait_text('persisted-yank');t.write(b'\x15\x19');wait_text('persisted-yank');t.close(preserve_draft=True);t=Terminal(binary,root,env);terminals.append(t);wait_text('persisted-yank');t.write(b'\x19');check('restart_retains_yanked_draft_but_clears_ephemeral_buffer','persisted-yank');t.clear_input()
            full='M'*(256*1024);paste(full);wait_text(full);t.write(b'\x15');wait_text('');paste('keep');wait_text('keep');before=draft();t.write(b'\x19');t.expect(b'Yank exceeds 256 KiB');check('yank_256k_rejection_atomic','keep');assert draft()==before
            t.write(b'\x7f'*4+b'\x19');check('rejected_yank_keeps_complete_buffer_for_retry',full);t.clear_input();t.close(preserve_draft=True)
            value=saved()
            for row in value['project_state']:row['messages']=[];row['draft'].update(input='',cursor=0,paste_folds=[],history=[])
            row=next(row for row in value['project_state'] if row['name']==value['projects'][value['active']]);row['messages']=[{'role':'System','text':'m'*(256*1024),'blocks':[]} for _ in range(15)]+[{'role':'System','text':'n'*(64*1024),'blocks':[]}]
            encoded=json.dumps(value,ensure_ascii=False,indent=2).encode();assert len(encoded)<4*1024*1024;checkpoint.write_bytes(encoded)
            t=Terminal(binary,root,env);terminals.append(t);wait_text('');paste('K'*150000);wait_text('K'*150000);t.write(b'\x15');wait_text('');paste('p'*75000);wait_text('p'*75000);before=draft();t.write(b'\x19');t.expect(b'Yank exceeds 4 MiB');check('pretty_checkpoint_yank_rejection_atomic','p'*75000);assert draft()==before
            t.write(b'\x7f\x19');check('checkpoint_rejection_preserves_buffer_retry_after_plain_fold_delete','K'*150000)
            result['journal_records']=records();t.close(preserve_draft=True);result.update(passed=True,request_count=len(requests),tui_processes=len(terminals))
    except Exception as error:result.update(passed=False,error=str(error));raise
    finally:
        for _,release in gates.values():release.set()
        for t in terminals:t.cleanup()
        server.shutdown();server.server_close();evidence.parent.mkdir(parents=True,exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals));evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n');evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--burst-only', action='store_true')
    parser.add_argument('--kill-yank-only', action='store_true')
    parser.add_argument('--large-paste-only', action='store_true')
    parser.add_argument('--queued-shell-paste-only', action='store_true')
    parser.add_argument('--evidence', type=Path, default=Path('Docs/quality/stage1/ux122-editor-pty.json'))
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.kill_yank_only:
        print(json.dumps(kill_yank_smoke(binary, args.evidence), indent=2))
        return
    if args.queued_shell_paste_only:
        print(json.dumps(queued_shell_paste_smoke(binary, args.evidence), indent=2))
        return
    if args.large_paste_only:
        print(json.dumps(large_paste_smoke(binary, args.evidence), indent=2))
        return
    if args.burst_only:
        print(json.dumps(burst_smoke(binary, args.evidence), indent=2))
        return
    requests = []
    gates = {name: (threading.Event(), threading.Event()) for name in ("SCHEDULE_GATE_ONE", "SCHEDULE_GATE_TWO", "ACTIVE_INPUT_GATE")}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            number = len(requests)
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            self.wfile.write(('event: response.created\ndata: '+json.dumps({'type':'response.created','response':{'id':f'composer-{number}','model':body['model']}})+'\n\n').encode())
            self.wfile.flush()
            latest = next((value for value in reversed(body.get('input', [])) if value.get('role') == 'user'), {})
            for marker, (started, release) in gates.items():
                if marker in json.dumps(latest):
                    started.set()
                    while not release.wait(.05):
                        try:
                            self.wfile.write(b'event: response.output_text.delta\ndata: {"type":"response.output_text.delta","delta":""}\n\n')
                            self.wfile.flush()
                        except (BrokenPipeError, ConnectionResetError):
                            return
            for kind, payload in [
                ('response.output_text.delta', {'delta': f'COMPOSER_REPLY_{number}'}),
                ('response.completed', {'response': {'id': f'composer-{number}', 'model': body['model'], 'status': 'completed'}}),
            ]:
                try:
                    self.wfile.write(('event: '+kind+'\ndata: '+json.dumps({'type': kind, **payload})+'\n\n').encode())
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    return

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}}
    terminals = []
    try:
        Path('.ops').mkdir(exist_ok=True)
        with TemporaryDirectory(prefix='composer-pty-', dir=Path('.ops').resolve()) as raw:
            root = Path(raw)
            config = root/'fixture'
            for path in [root/'initial', root/'sessions', config]:
                path.mkdir(parents=True)
            (config/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config/'auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'composer-fixture'}))
            (config/'auth.json').chmod(0o600)
            env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
            env.update(ZENPI_HOME=str(config), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
            terminal = Terminal(binary, root, env)
            terminals.append(terminal)
            checkpoint = config/'project-tabs.json'

            def draft():
                value = json.loads(checkpoint.read_text())
                active = value['projects'][value['active']]
                return next(row['draft']['input'] for row in value['project_state'] if row['name'] == active)

            def ready(number):
                terminal.expect(f'COMPOSER_REPLY_{number}'.encode())
                terminal.wait(lambda: ' Ready |' in screen_text(bytes(terminal.output)).decode().splitlines()[1])
                assert len(requests) == number

            def paste(text):
                terminal.write(b'\x1b[200~'+text.encode()+b'\x1b[201~')

            terminal.command('history alpha old')
            ready(1)
            terminal.command('history beta')
            ready(2)
            terminal.command('history alpha new')
            ready(3)
            paste('original draft 世界')
            terminal.wait(lambda: draft() == 'original draft 世界')
            terminal.write(b'\x12alpha')
            terminal.expect(b'History search: alpha')
            terminal.expect(b'history alpha new')
            assert len(requests) == 3
            terminal.write(b'\x12')
            terminal.expect(b'history alpha old')
            terminal.cancel_picker()
            terminal.wait(lambda: draft() == 'original draft 世界')
            result['checks']['history_search_cancel_restores_draft_without_request'] = True
            terminal.write(b'\x12beta\r')
            terminal.wait(lambda: draft() == 'history beta')
            assert len(requests) == 3
            terminal.write(b'\r')
            ready(4)
            latest = next(v for v in reversed(requests[-1]['input']) if v.get('role') == 'user')
            assert 'history beta' in json.dumps(latest)
            result['checks']['history_accept_then_explicit_submit'] = True
            paste('draft survives history')
            terminal.write(b'\x10')
            terminal.expect(b'history beta')
            terminal.write(b'\x0e')
            terminal.wait(lambda: draft() == 'draft survives history')
            terminal.cancel_picker()
            terminal.wait(lambda: draft() == 'draft survives history')
            result['checks']['history_browse_and_escape_keep_draft'] = True
            terminal.write(b'\x15')
            paste('a👨‍👩‍👧‍👦e\u0301🇯🇵z')
            terminal.write(b'\x1b[D\x7f\x7f\x1b[D\x1b[3~')
            terminal.wait(lambda: draft() == 'az')
            result['checks']['actual_grapheme_cursor_backspace_delete'] = True
            terminal.write(b'\x15\x05\x15')
            paste('alpha 世界 beta')
            terminal.write(b'\x1b[1;5D\x17')
            terminal.wait(lambda: draft() == 'alpha beta')
            terminal.write(b'\x05\x0aline two')
            terminal.wait(lambda: draft() == 'alpha beta\nline two')
            result['checks']['word_deletion_and_portable_multiline'] = True
            terminal.clear_input()
            literal = '/exit\n!touch pasted-command-must-not-run\nlast'
            paste(literal)
            terminal.wait(lambda: draft() == literal)
            assert len(requests) == 4
            assert not (root/'initial/pasted-command-must-not-run').exists()
            terminal.cancel_picker()
            terminal.wait(lambda: draft() == literal)
            result['checks']['bracketed_paste_never_executes_shortcuts_or_commands'] = True
            terminal.close(preserve_draft=True)
            terminal = Terminal(binary, root, env)
            terminals.append(terminal)
            terminal.wait(lambda: draft() == literal)
            terminal.clear_input()
            terminal.write(b'\x12beta')
            terminal.expect(b'History search: beta')
            terminal.write(b'\r')
            terminal.wait(lambda: draft() == 'history beta')
            assert len(requests) == 4
            result['checks']['independent_process_draft_and_history_restore'] = True
            terminal.write(b'\x15')
            terminal.command('SCHEDULE_GATE_ONE')
            terminal.wait(gates['SCHEDULE_GATE_ONE'][0].is_set)
            terminal.command("!printf original > scheduled-edited.txt")
            terminal.wait(lambda: len(json.loads(checkpoint.read_text())['scheduled_inputs']) == 1)
            pending = json.loads(checkpoint.read_text())['scheduled_inputs'][0]
            scheduled_id = pending['id']
            original_project = pending['project']
            terminal.command('/scheduled list')
            terminal.expect(b'Scheduled jobs')
            terminal.expect(b'"status": "scheduled"')
            terminal.command(f'/scheduled edit {scheduled_id} 0 printf edited > scheduled-edited.txt')
            terminal.wait(lambda: json.loads(checkpoint.read_text())['scheduled_inputs'][0]['revision'] == 1)
            terminal.command(f'/scheduled edit {scheduled_id} 0 stale payload')
            terminal.wait(lambda: draft().endswith('stale payload'))
            terminal.write(b'\x15')
            terminal.command('!touch scheduled-cancelled.txt')
            terminal.wait(lambda: len(json.loads(checkpoint.read_text())['scheduled_inputs']) == 2)
            cancelled_id = json.loads(checkpoint.read_text())['scheduled_inputs'][1]['id']
            terminal.command(f'/scheduled cancel {cancelled_id}')
            terminal.wait(lambda: len(json.loads(checkpoint.read_text())['scheduled_inputs']) == 1)
            assert not (root/'initial/scheduled-edited.txt').exists()
            assert not (root/'initial/scheduled-cancelled.txt').exists()
            assert len(requests) == 5
            other = root/'other'
            other.mkdir()
            terminal.folder(other, mouse=False)
            terminal.wait(lambda: json.loads(checkpoint.read_text())['active'] == 1)
            terminal.command(f'/scheduled cancel {scheduled_id}')
            terminal.wait(lambda: draft() == f'/scheduled cancel {scheduled_id}')
            terminal.expect(b'Scheduled job belongs to another project')
            assert len(json.loads(checkpoint.read_text())['scheduled_inputs']) == 1
            terminal.write(b'\x15')
            terminal.command(f'/project select {original_project}')
            terminal.wait(lambda: json.loads(checkpoint.read_text())['active'] == 0)
            gates['SCHEDULE_GATE_ONE'][1].set()
            terminal.wait(lambda: 'Approval required' in screen_text(bytes(terminal.output)).decode().splitlines()[1])
            assert not (root/'initial/scheduled-edited.txt').exists()
            terminal.write(b'y\r')
            terminal.wait(lambda: (root/'initial/scheduled-edited.txt').read_text() == 'edited')
            terminal.wait(lambda: 'Local shell exit 0' in screen_text(bytes(terminal.output)).decode().splitlines()[1])
            assert not (root/'initial/scheduled-cancelled.txt').exists()
            terminal.command(f'/scheduled edit {scheduled_id} 1 after-start')
            terminal.wait(lambda: draft().endswith('after-start'))
            terminal.expect(b'Scheduled job missing or already started')
            terminal.write(b'\x15')
            result['checks']['scheduled_edit_cancel_revision_project_and_started_guard'] = True
            result['checks']['scheduled_actual_edited_shell_after_explicit_approval'] = True
            terminal.command('SCHEDULE_GATE_TWO')
            terminal.wait(gates['SCHEDULE_GATE_TWO'][0].is_set)
            terminal.command('!touch scheduled-restart-must-not-run.txt')
            terminal.wait(lambda: len(json.loads(checkpoint.read_text())['scheduled_inputs']) == 1)
            restart_id = json.loads(checkpoint.read_text())['scheduled_inputs'][0]['id']
            terminal.close(preserve_draft=True)
            gates['SCHEDULE_GATE_TWO'][1].set()
            terminal = Terminal(binary, root, env)
            terminals.append(terminal)
            terminal.command('/scheduled list')
            terminal.expect(b'interrupted_unconfirmed')
            assert not (root/'initial/scheduled-restart-must-not-run.txt').exists()
            assert len(requests) == 6
            terminal.command(f'/scheduled cancel {restart_id}')
            terminal.wait(lambda: not json.loads(checkpoint.read_text())['scheduled_inputs'])
            result['checks']['scheduled_restart_requires_explicit_retry_and_never_autoruns'] = True
            terminal.command('ACTIVE_INPUT_GATE')
            terminal.wait(gates['ACTIVE_INPUT_GATE'][0].is_set)
            journal = root/'sessions/initial.jsonl'

            def input_events():
                return [record['event'] for record in (json.loads(line) for line in journal.read_text().splitlines()) if record.get('kind') == 'event' and record['event'].get('type') == 'input_queue']

            def changed(action, input_id=None):
                return [event['change'] for event in input_events() if event['change']['action'] == action and (input_id is None or event['change'].get('id', event['change'].get('input', {}).get('id')) == input_id)]

            terminal.command('auto queued original')
            terminal.wait(lambda: any(event['change'].get('input', {}).get('text') == 'auto queued original' for event in input_events()))
            auto_id = next(event['change']['input']['id'] for event in input_events() if event['change'].get('input', {}).get('text') == 'auto queued original')
            terminal.expect(b'Input owner confirmed')
            assert len(requests) == 7
            terminal.command('/input list')
            terminal.expect(auto_id.encode())
            terminal.write(f'/input edit {auto_id}'.encode())
            terminal.write(b'\t')
            terminal.wait(lambda: draft().startswith(f'/input edit {auto_id} 0 auto queued original'))
            terminal.write(b'\x17edited'); terminal.deliberate_enter()
            terminal.wait(lambda: changed('edited', auto_id))
            assert changed('edited', auto_id)[-1]['text'] == 'auto queued edited'
            terminal.command(f'/input edit {auto_id} 0 stale')
            # The same draft also exists before Enter is handled. Observe the
            # asynchronous owner rejection before treating it as restored.
            terminal.expect(b'Input update rejected: input edit revision is stale')
            terminal.wait(lambda: draft() == f'/input edit {auto_id} 0 stale')
            assert len(changed('edited', auto_id)) == 1
            terminal.write(b'\x15')
            terminal.command('/input follow-up removed-input must-not-apply')
            terminal.wait(lambda: changed('enqueued', 'removed-input'))
            terminal.command('/input cancel removed-input')
            terminal.wait(lambda: changed('cancelled', 'removed-input'))
            terminal.command('/input follow-up final-input final-follow-up-text')
            terminal.wait(lambda: changed('enqueued', 'final-input'))
            terminal.command('/input mode steer all')
            terminal.wait(lambda: changed('configured'))
            terminal.command('/input steer forbidden-file inspect @file.txt')
            terminal.wait(lambda: draft() == '/input steer forbidden-file inspect @file.txt')
            terminal.expect(b'text-only')
            assert not changed('enqueued', 'forbidden-file')
            terminal.write(b'\x15')
            terminal.command(f'/project select {json.loads(checkpoint.read_text())["projects"][1]}')
            terminal.wait(lambda: json.loads(checkpoint.read_text())['active'] == 1)
            terminal.command(f'/input cancel {auto_id}')
            terminal.wait(lambda: draft() == f'/input cancel {auto_id}')
            terminal.expect(b'unknown input ID')
            assert not changed('cancelled', auto_id)
            terminal.write(b'\x15')
            terminal.command(f'/project select {original_project}')
            terminal.wait(lambda: json.loads(checkpoint.read_text())['active'] == 0)
            assert len(requests) == 7
            assert not changed('applied')
            gates['ACTIVE_INPUT_GATE'][1].set()
            ready(9)
            applied = changed('applied')
            assert sum(auto_id in row['ids'] for row in applied) == 1
            assert sum('final-input' in row['ids'] for row in applied) == 1
            assert not any('removed-input' in row['ids'] for row in applied)
            assert all(row['would_stop'] for row in applied if 'final-input' in row['ids'])
            assert 'auto queued edited' in json.dumps(requests[7])
            assert 'must-not-apply' not in json.dumps(requests[7:])
            result['checks']['active_input_durable_receipts_edit_cancel_modes_and_boundary'] = True
            result['checks']['active_input_cross_project_and_attachment_rejections'] = True
            result['checks']['active_input_does_not_cancel_or_reissue_original_turn'] = True
            terminal.close()
            terminal = Terminal(binary, root, env)
            terminals.append(terminal)
            terminal.command('/input list')
            terminal.expect(auto_id.encode())
            terminal.expect(b'applied')
            terminal.expect(b'cancelled')
            assert len(requests) == 9
            result['checks']['input_queue_independent_process_restore_no_automatic_resend'] = True
            terminal.close()
            result['checks']['terminal_restored'] = True
            result['request_count'] = len(requests)
            result['passed'] = True
    finally:
        for started, release in gates.values():
            release.set()
        server.shutdown()
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        args.evidence.with_suffix('.pty.log').write_bytes(b'\nNEXT PROCESS\n'.join(bytes(t.output) for t in terminals))
        args.evidence.write_text(json.dumps(result, indent=2)+'\n')
        for terminal in terminals:
            if terminal.pid:
                os.close(terminal.fd)
                try:
                    os.kill(terminal.pid, 9)
                    os.waitpid(terminal.pid, 0)
                except (ProcessLookupError, ChildProcessError):
                    pass
                terminal.pid = None
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()

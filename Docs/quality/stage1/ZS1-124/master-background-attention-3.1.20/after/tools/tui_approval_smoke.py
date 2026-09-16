#!/usr/bin/env python3
"""Production PTY approval focus, actual write boundaries, durable policy and restart."""
from __future__ import annotations
import argparse, hashlib, json, os, threading, time, signal
from pathlib import Path
from tempfile import TemporaryDirectory
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from tui_project_workspace_smoke import Terminal, screen_text

def selector_smoke(binary, evidence):
    """Bare policy commands must open a cancellable selector before mutation."""
    requests, terminals, failures = [], [], []
    connections = []
    seen = set()
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'checks': {}, 'observations': []}

    class Handler(BaseHTTPRequestHandler):
        def handle(self):
            connections.append({'client': self.client_address})
            super().handle()

        def log_message(self, *args):
            connections.append({'log': args})

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            latest = next(row for row in reversed(body['input']) if row.get('role') == 'user')
            text = json.dumps(latest)
            marker = next(m for m in ('CANCEL_0', 'SELECT_0', 'CANCEL_1', 'SELECT_1') if m in text)
            events = [{'type': 'response.created', 'response': {'id': marker, 'model': body['model']}}]
            if marker not in seen:
                seen.add(marker)
                events.append({'type': 'response.output_item.done', 'item': {
                    'type': 'function_call', 'call_id': marker, 'name': 'write_file',
                    'arguments': json.dumps({'path': marker + '.txt', 'content': marker})}})
            else:
                events.append({'type': 'response.output_text.delta', 'delta': 'DONE_' + marker})
            events.append({'type': 'response.completed', 'response': {
                'id': marker, 'model': body['model'], 'status': 'completed'}})
            raw = ''.join('data: ' + json.dumps(event) + '\n\n' for event in events).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    evidence = evidence.resolve()
    fixture = evidence.with_suffix('.fixture')
    fixture.mkdir(parents=True)

    def check(name, value):
        result['checks'][name] = bool(value)
        if not value:
            failures.append(name)

    try:
        for index, alias in enumerate(('/approval', '/approvals')):
            root = fixture / str(index)
            config = root / 'fixture'
            for path in (root / 'initial', root / 'sessions', config):
                path.mkdir(parents=True)
            (config / 'config.toml').write_text(
                f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config / 'auth.json').write_text('{"OPENAI_API_KEY":"policy-selector-fixture"}\n')
            (config / 'auth.json').chmod(0o600)
            env = {k: v for k, v in os.environ.items()
                   if not k.startswith(('ZENPI_', 'OPENAI_'))
                   and k.lower() not in ('http_proxy', 'https_proxy', 'all_proxy', 'no_proxy')}
            env.update(ZENPI_HOME=str(config), TERM='xterm-256color')
            t = Terminal(binary, root, env)
            terminals.append(t)
            checkpoint = config / 'project-tabs.json'

            def active():
                saved = json.loads(checkpoint.read_text())
                return next(row for row in saved['project_state'] if row['name'] == saved['projects'][saved['active']])

            def mode_messages():
                return json.dumps(active()['messages']).count('approval mode:')

            def idle():
                t.wait(lambda: b'Ready' in screen_text(bytes(t.output)).splitlines()[1])

            def set_mode(mode):
                t.clear_input()
                count = mode_messages()
                t.command(alias + ' ' + mode)
                t.wait(lambda: mode_messages() == count + 1 and t.draft() == '')

            def open_selector(label):
                count, http_count = mode_messages(), len(requests)
                owner = active()['metadata']
                layout = active()['layout']
                t.type_text(alias)
                t.wait(lambda: t.draft() == alias)
                t.write(b'\r')
                t.wait(lambda: t.draft() != alias)
                screen = screen_text(bytes(t.output)).decode()
                opened = t.draft() == alias + ' ' and all(alias + ' ' + mode in screen for mode in ('ask', 'always', 'never'))
                result['observations'].append({'alias': alias, 'stage': label,
                    'draft': t.draft(), 'mode_messages_before': count,
                    'mode_messages_after': mode_messages(), 'screen': screen})
                check(label + '_opens_without_policy_or_http_change',
                      opened and mode_messages() == count and len(requests) == http_count)
                check(label + '_preserves_project_session_layout',
                      active()['metadata'] == owner and active()['layout'] == layout)
                return opened, count

            set_mode('always')
            opened, count = open_selector(str(index) + '_always')
            if opened:
                t.cancel_picker()
            check(str(index) + '_escape_keeps_always_and_command_draft',
                  t.draft() == alias + ' ' and mode_messages() == count)
            set_mode('never')
            opened, count = open_selector(str(index) + '_never')
            if opened:
                t.cancel_picker()
            check(str(index) + '_escape_keeps_never_and_command_draft',
                  t.draft() == alias + ' ' and mode_messages() == count)
            t.clear_input()
            marker = 'CANCEL_' + str(index)
            target = root / 'initial' / (marker + '.txt')
            t.command(marker)
            t.wait(lambda: target.exists() or b'Enter confirm' in screen_text(bytes(t.output)))
            check(str(index) + '_cancelled_selector_retains_actual_no_prompt_write_policy', target.exists())
            if not target.exists():
                t.write(b'n\r')
            t.expect(('DONE_' + marker).encode())
            idle()

            # Continue the before run for both spellings; a missing submenu is
            # a retained failure, never repaired by injecting a trailing space.
            opened, count = open_selector(str(index) + '_select')
            if opened:
                t.write(b'\x1b[B\r')
                t.wait(lambda: t.draft() == alias + ' always ')
                check(str(index) + '_highlight_requires_separate_confirmation', mode_messages() == count)
                t.write(b'\r')
                t.wait(lambda: mode_messages() == count + 1 and t.draft() == '')
                marker = 'SELECT_' + str(index)
                target = root / 'initial' / (marker + '.txt')
                t.command(marker)
                t.expect(('Call: ' + marker).encode())
                t.expect(b'Enter confirm')
                check(str(index) + '_explicit_always_selection_reaches_real_approval_owner', not target.exists())
                t.write(b'n\r')
                t.expect(('DONE_' + marker).encode())
                idle()
                check(str(index) + '_denied_write_stays_absent', not target.exists())
            t.close()
            check(str(index) + '_terminal_restored', b'\x1b[?1049l' in t.output)
        result.update(status='failed' if failures else 'passed', failures=failures,
                      request_count=len(requests), tui_processes=len(terminals))
        assert not failures, failures
    except BaseException as error:
        result.update(status='failed', error=repr(error))
        raise
    finally:
        for terminal in terminals:
            terminal.cleanup()
        server.shutdown()
        server.server_close()
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.http.json').write_text(json.dumps(requests, indent=2) + '\n')
        result['server_connections'] = connections
        evidence.write_text(json.dumps(result, indent=2) + '\n')
    return result

def background_smoke(binary, evidence, expect_missing=False):
    """Delay a real provider until its project has become inactive."""
    evidence = evidence.resolve()
    root = evidence.with_suffix('.fixture')
    config = root / 'fixture'
    other = root / 'other project'
    for path in (root / 'initial', root / 'sessions', config, other):
        path.mkdir(parents=True)
    entered, release = threading.Event(), threading.Event()
    requests, errors = [], []
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}}
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            try:
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                requests.append(body)
                finished = any(row.get('type') == 'function_call_output' for row in body['input'])
                if not finished:
                    entered.set()
                    assert release.wait(10), 'provider not released'
                    events = [{'type':'response.output_item.done', 'item':{
                        'type':'function_call', 'call_id':'background-write', 'name':'write_file',
                        'arguments':json.dumps({'path':'approved.txt', 'content':'EXPLICIT_BACKGROUND_APPROVAL'})}}]
                else:
                    events = [{'type':'response.output_text.delta', 'delta':'BACKGROUND_APPROVAL_DONE'}]
                events.append({'type':'response.completed', 'response':{
                    'id':'background', 'model':body['model'], 'status':'completed'}})
                raw = ''.join('data: '+json.dumps(e)+'\n\n' for e in events).encode()
                self.send_response(200)
                self.send_header('Content-Type','text/event-stream')
                self.send_header('Content-Length',str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
            except BaseException as error:
                errors.append(repr(error))
                raise
    server = ThreadingHTTPServer(('127.0.0.1',0), Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    (config/'config.toml').write_text(
        f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
        'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
    (config/'auth.json').write_text('{"OPENAI_API_KEY":"background-fixture"}\n')
    (config/'auth.json').chmod(0o600)
    env = {k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))
           and k.lower() not in ('http_proxy','https_proxy','all_proxy','no_proxy')}
    env.update(ZENPI_HOME=str(config),TERM='xterm-256color')
    t = None
    def saved(): return json.loads((config/'project-tabs.json').read_text())
    def active():
        data=saved();return next(row for row in data['project_state'] if row['name']==data['projects'][data['active']])
    def screen(): return screen_text(bytes(t.output)).decode()
    try:
        t=Terminal(binary,root,env)
        t.command('BACKGROUND_APPROVAL_REQUEST')
        t.wait(entered.is_set)
        origin=active()['name']
        t.folder(other)
        t.wait(lambda:active()['metadata']['cwd']==str(other.resolve()))
        layout=active()['layout']
        draft='other project draft 中文'
        t.write(draft.encode())
        t.wait(lambda:t.draft()==draft)
        release.set()
        t.wait(lambda:any('Approval required' in str(row['messages']) for row in saved()['project_state'] if row['name']==origin))
        # Await an actual redraw after the durable background message appears.
        deadline=time.monotonic()+.3
        while time.monotonic()<deadline:
            from tui_user_shell_smoke import drain
            drain(t.fd,t.output,.02)
        view=screen(); lines=view.splitlines()
        result['pending_screen']=view
        badge='!1' in lines[0] and 'initial' in lines[0]
        hint='Approval waiting: initial (1)' in lines[-1] and 'Alt-A' in lines[-1]
        result['observations']={'source_tab_badge':badge,'background_source_and_review_hint':hint}
        assert not (root/'initial/approved.txt').exists() and not (other/'approved.txt').exists()
        assert len(requests)==1 and t.draft()==draft and active()['layout']==layout
        result['checks']['actual_background_approval_preserves_active_draft_layout_and_has_no_write']=True
        if expect_missing:
            assert not badge and not hint, result
            result['status']='observed_missing'
            return result
        assert badge and hint, result
        result['checks']['pending_origin_badge_and_footer_appear_without_changing_active_project']=True
        t.folder(root/'initial')
        t.wait(lambda:active()['name']==origin)
        t.write(b'\x1ba')
        t.expect(b'Enter confirm')
        assert not (root/'initial/approved.txt').exists()
        t.write(b'y\r')
        t.wait(lambda:(root/'initial/approved.txt').exists())
        t.expect(b'BACKGROUND_APPROVAL_DONE')
        t.wait(lambda:'!1' not in screen().splitlines()[0])
        assert (root/'initial/approved.txt').read_text()=='EXPLICIT_BACKGROUND_APPROVAL'
        assert not (other/'approved.txt').exists() and len(requests)==2
        result['checks']['switch_back_and_explicit_review_writes_only_origin_then_clears_badge']=True
        t.folder(other)
        t.wait(lambda:active()['metadata']['cwd']==str(other.resolve()) and t.draft()==draft)
        assert active()['layout']==layout and 'Approval waiting:' not in screen().splitlines()[-1]
        result['checks']['other_project_draft_layout_survive_and_resolved_notice_disappears']=True
        t.close(preserve_draft=True)
        assert not errors,errors
        result.update(status='passed',tui_processes=1,request_count=len(requests))
    except BaseException as error:
        result.update(status='failed',error=repr(error))
        raise
    finally:
        release.set()
        if t:
            t.cleanup()
            evidence.with_suffix('.pty.log').write_bytes(bytes(t.output))
        server.shutdown();server.server_close()
        evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n')
        result['server_errors']=errors
        evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--evidence', type=Path, default=Path('Docs/quality/stage1/ux124-pty-result.json'))
    parser.add_argument('--selector-only', action='store_true')
    parser.add_argument('--background-only', action='store_true')
    parser.add_argument('--expect-missing-background', action='store_true')
    args = parser.parse_args()
    binary = args.binary.resolve()
    if args.expect_missing_background and not args.background_only:
        parser.error('--expect-missing-background requires --background-only')
    if args.background_only:
        if args.selector_only:
            parser.error('choose either --background-only or --selector-only')
        print(json.dumps(background_smoke(binary, args.evidence, args.expect_missing_background), indent=2))
        return
    if args.selector_only:
        print(json.dumps(selector_smoke(binary, args.evidence), indent=2))
        return
    requests = []
    seen = set()
    terminals = []

    class Handler(BaseHTTPRequestHandler):

        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            latest = next((row for row in reversed(body.get('input', [])) if row.get('role') == 'user'), {})
            marker = next((m for m in ['ALLOW_CASE', 'DENY_CASE', 'CANCEL_CASE', 'MULTI_CASE', 'RESTART_PENDING', 'REMEMBER_DENY_SECOND', 'REMEMBER_DENY'] if m in json.dumps(latest)), None)
            events = [{'type': 'response.created', 'response': {'id': f'approval-{len(requests)}', 'model': body['model']}}]
            if marker and marker not in seen:
                seen.add(marker)
                names = ['MULTI_ONE', 'MULTI_TWO'] if marker == 'MULTI_CASE' else [marker]
                for name in names:
                    events.append({'type': 'response.output_item.done', 'item': {'type': 'function_call', 'call_id': name, 'name': 'write_file', 'arguments': json.dumps({'path': name + '.txt', 'content': 'approved ' + name + '\n' + '\n'.join((f'line {i}' for i in range(80)))})}})
            else:
                events.append({'type': 'response.output_text.delta', 'delta': 'APPROVAL_DONE_' + str(marker)})
            events.append({'type': 'response.completed', 'response': {'id': f'approval-{len(requests)}', 'model': body['model'], 'status': 'completed'}})
            raw = ''.join(('data: ' + json.dumps(e) + '\n\n' for e in events)).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks': {}}
    try:
        with TemporaryDirectory(prefix='approval-pty-', dir=Path('.ops').resolve()) as raw:
            root = Path(raw)
            config = root / 'fixture'
            for p in [root / 'initial', root / 'sessions', root / 'other', config]:
                p.mkdir(parents=True)
            (config / 'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
            (config / 'auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'approval-fixture'}))
            (config / 'auth.json').chmod(0o600)
            env = {k: v for (k, v) in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
            env.update(ZENPI_HOME=str(config), TERM='xterm-256color')
            t = Terminal(binary, root, env)
            terminals.append(t)
            session = root / 'sessions/initial.jsonl'
            checkpoint = config / 'project-tabs.json'

            def records():
                return [json.loads(line) for line in session.read_text().splitlines() if line.strip()]

            def receipts():
                return [r['event'] for r in records() if r.get('event', {}).get('type') == 'approval_resolved']

            def draft():
                c = json.loads(checkpoint.read_text())
                name = c['projects'][c['active']]
                return next((r['draft']['input'] for r in c['project_state'] if r['name'] == name))

            def idle():
                t.wait(lambda : any((word in screen_text(bytes(t.output)).splitlines()[1] for word in (b'Ready', b'Interrupted'))))

            def review(marker):
                idle()
                t.command(marker)
                t.expect(b'Enter confirm')
                t.expect(('Call: ' + ('MULTI_ONE' if marker == 'MULTI_CASE' else marker)).encode())
                assert not (root / 'initial' / f'{marker}.txt').exists()
            t.write(b'\x0f')
            t.expect(b'folded')
            review('ALLOW_CASE')
            result['checks']['unapproved_write_absent_and_folded_diff_visible'] = True
            t.write(b'\x1b[F')
            t.expect(b'line 79')
            t.write(b'\x1b[H')
            t.expect(b'Call: ALLOW_CASE')
            result['checks']['full_diff_scrolling'] = True
            t.cancel_picker()
            t.write(b'preserved draft')
            t.wait(lambda : draft() == 'preserved draft')
            t.write(b'\x1ba')
            t.expect(b'Enter confirm')
            t.write(b'y\r')
            t.wait(lambda : (root / 'initial/ALLOW_CASE.txt').exists())
            t.wait(lambda : any((r.get('turn', {}).get('content', '').startswith('APPROVAL_DONE_ALLOW_CASE') for r in records())))
            t.wait(lambda : draft() == 'preserved draft')
            assert receipts()[-1]['decision'] == 'allow'
            result['checks']['allow_actual_write_and_draft_cursor_focus_restore'] = True
            rs = records()
            audit = next((i for (i, r) in enumerate(rs) if r.get('event', {}).get('type') == 'approval_resolved'))
            tool = next((i for (i, r) in enumerate(rs) if r.get('turn', {}).get('role') == 'tool'))
            assert audit < tool
            result['checks']['audit_precedes_tool_result'] = True
            t.write(b'\x15')
            review('DENY_CASE')
            t.write(b'\x1b[200~y\n/approve fake always\x1b[201~')
            t.wait(lambda : 'y\n/approve fake always' in draft())
            assert not (root / 'initial/DENY_CASE.txt').exists()
            result['checks']['paste_never_decides'] = True
            t.clear_input()
            t.write(b'\x1ba')
            t.expect(b'Enter confirm')
            t.write(b'n\r')
            t.wait(lambda : any((r.get('call_id') == 'DENY_CASE' and r.get('decision') == 'deny' for r in receipts())))
            assert not (root / 'initial/DENY_CASE.txt').exists()
            result['checks']['deny_durable_no_write'] = True
            t.write(b'\x05\x15')
            t.wait(lambda : draft() == '')
            review('CANCEL_CASE')
            t.folder(root / 'other')
            t.wait(lambda: json.loads(checkpoint.read_text())['active'] == 1)
            t.expect(b'other')
            result['checks']['project_picker_takes_focus_over_approval'] = True
            origin = json.loads(checkpoint.read_text())['projects'][0]
            t.command('/approve stale once')
            t.expect(b'another project')
            assert not (root / 'initial/CANCEL_CASE.txt').exists()
            result['checks']['cross_project_approval_rejected'] = True
            t.command('/project select ' + origin)
            t.write(b'\x1ba')
            t.expect(b'Enter confirm')
            t.write(b'\x03')
            t.expect(b'Interrupted')
            assert not (root / 'initial/CANCEL_CASE.txt').exists()
            result['checks']['cancel_pending_no_write'] = True
            review('MULTI_CASE')
            t.expect(b'MULTI_ONE')
            t.write(b'y\r')
            t.expect(b'Call: MULTI_TWO')
            assert (root / 'initial/MULTI_ONE.txt').exists() and (not (root / 'initial/MULTI_TWO.txt').exists())
            t.write(b'y\r')
            t.wait(lambda : (root / 'initial/MULTI_ONE.txt').exists() and (root / 'initial/MULTI_TWO.txt').exists())
            t.expect(b'APPROVAL_DONE_MULTI_CASE')
            result['checks']['sequential_real_requests_exact_call_decisions'] = True
            review('RESTART_PENDING')
            before_restart = len(requests)
            t.close(preserve_draft=True)
            t = Terminal(binary, root, env)
            terminals.append(t)
            idle()
            assert len(requests) == before_restart and (not (root / 'initial/RESTART_PENDING.txt').exists())
            assert b'Enter confirm' not in screen_text(bytes(t.output))
            result['checks']['unfinished_approval_restart_never_replays_or_grants'] = True
            review('REMEMBER_DENY')
            t.write(b'nr\r')
            t.wait(lambda : any((r.get('call_id') == 'REMEMBER_DENY' and r.get('remember') is True for r in receipts())))
            assert not (root / 'initial/REMEMBER_DENY.txt').exists()
            result['checks']['remembered_deny_durable'] = True
            idle()
            t.close()
            t = Terminal(binary, root, env)
            terminals.append(t)
            before = len(requests)
            t.command('REMEMBER_DENY_SECOND')
            t.wait(lambda : len(requests) > before)
            t.expect(b'APPROVAL_DONE_REMEMBER_DENY_SECOND')
            assert any((row.get('call_id') == 'REMEMBER_DENY_SECOND' and 'policy_denied' in row.get('output', '') for request in requests for row in request.get('input', []) if row.get('type') == 'function_call_output'))
            assert not (root / 'initial/REMEMBER_DENY_SECOND.txt').exists()
            assert not any((r.get('call_id') == 'REMEMBER_DENY_SECOND' for r in receipts()))
            result['checks']['independent_restart_remembered_policy_no_prompt_or_write'] = True
            t.close()
            result['checks']['terminal_restored'] = True
            result['request_count'] = len(requests)
            result['passed'] = True
    except BaseException as error:
        result['error'] = repr(error)
        raise
    finally:
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        args.evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join((bytes(t.output) for t in terminals)))
        args.evidence.write_text(json.dumps(result, indent=2) + '\n')
        for t in terminals:
            if t.pid:
                try:
                    os.kill(t.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                deadline = time.monotonic() + 2
                while time.monotonic() < deadline:
                    try:
                        if os.waitpid(t.pid, os.WNOHANG)[0]:
                            break
                    except ChildProcessError:
                        break
                    time.sleep(0.05)
                os.close(t.fd)
                t.pid = None
        server.shutdown()
        server.server_close()
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        args.evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join((bytes(t.output) for t in terminals)))
        args.evidence.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
if __name__ == '__main__':
    main()

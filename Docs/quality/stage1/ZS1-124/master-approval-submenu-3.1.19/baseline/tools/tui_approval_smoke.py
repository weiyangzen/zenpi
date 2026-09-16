#!/usr/bin/env python3
"""Production PTY approval focus, actual write boundaries, durable policy and restart."""
from __future__ import annotations
import argparse, hashlib, json, os, threading, time, signal
from pathlib import Path
from tempfile import TemporaryDirectory
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from tui_project_workspace_smoke import Terminal, screen_text

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--evidence', type=Path, default=Path('Docs/quality/stage1/ux124-pty-result.json'))
    args = parser.parse_args()
    binary = args.binary.resolve()
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

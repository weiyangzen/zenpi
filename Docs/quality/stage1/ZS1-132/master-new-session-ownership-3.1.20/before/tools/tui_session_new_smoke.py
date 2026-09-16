#!/usr/bin/env python3
"""Actual TUI and JSONL /new identity, recovery and cancellation checks."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import queue
import signal
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from tui_project_workspace_smoke import Terminal


def before_smoke(binary, evidence):
    requests = []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            requests.append(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
            events = [
                {'type': 'response.output_text.delta', 'delta': 'OLD_SESSION_REPLY'},
                {'type': 'response.completed', 'response': {'id': 'old', 'model': 'gpt-4.1', 'status': 'completed'}},
            ]
            raw = ''.join('data: ' + json.dumps(e) + '\n\n' for e in events).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

    evidence = evidence.resolve()
    root = evidence.with_suffix('.fixture') / '工作 space'
    config = root / 'fixture'
    for path in (root / 'initial', root / 'sessions', config):
        path.mkdir(parents=True)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    (config / 'config.toml').write_text(
        f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
        'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
    (config / 'auth.json').write_text('{"OPENAI_API_KEY":"session-new-fixture"}\n')
    (config / 'auth.json').chmod(0o600)
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('ZENPI_', 'OPENAI_'))
           and k.lower() not in ('http_proxy', 'https_proxy', 'all_proxy', 'no_proxy')}
    env.update(ZENPI_HOME=str(config), TERM='xterm-256color')
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'scope': 'before: actual missing /new, not an implementation pass', 'checks': {}}
    terminal = None
    try:
        terminal = Terminal(binary, root, env)
        terminal.command('OLD_SESSION_SEED')
        terminal.expect(b'OLD_SESSION_REPLY')
        terminal.wait(lambda: terminal.draft() == '')
        session = root / 'sessions/initial.jsonl'
        original = session.read_bytes()
        header = json.loads(original.splitlines()[0])
        terminal.type_text('/new')
        terminal.deliberate_enter()
        terminal.expect(b'unknown slash command')
        terminal.wait(lambda: terminal.draft() == '/new')
        assert len(requests) == 1
        assert session.read_bytes().startswith(original)
        result['checks']['actual_tui_new_missing_preserves_old_owner_and_draft'] = True
        terminal.close(preserve_draft=True)
        rows = [{'schema_version': 2, 'id': 'new-before', 'type': 'command', 'text': '/new'},
                {'schema_version': 2, 'id': 'stop', 'type': 'shutdown'}]
        run = subprocess.run([str(binary), '--mode', 'headless', '--backend', 'openai', '--session', str(session)],
                             input=''.join(json.dumps(r) + '\n' for r in rows), capture_output=True,
                             text=True, cwd=root / 'initial', env=env, timeout=15)
        evidence.with_suffix('.jsonl.stdout').write_text(run.stdout)
        evidence.with_suffix('.jsonl.stderr').write_text(run.stderr)
        assert run.returncode == 0, run.stderr
        events = [json.loads(line) for line in run.stdout.splitlines() if line]
        response = next(event for event in events if event.get('id') == 'new-before' and event.get('type') == 'response')
        assert response['success'] is False and 'unknown' in json.dumps(response).lower(), response
        assert json.loads(session.read_bytes().splitlines()[0]) == header
        assert session.read_bytes().startswith(original) and len(requests) == 1
        result['checks']['actual_jsonl_new_missing_preserves_identity_and_history'] = True
        result.update(status='observed_missing', response=response, request_count=len(requests), tui_processes=1, jsonl_processes=1)
    except BaseException as error:
        result.update(status='fixture_failed', error=repr(error))
        raise
    finally:
        if terminal:
            terminal.cleanup()
            evidence.with_suffix('.pty.log').write_bytes(bytes(terminal.output))
        server.shutdown()
        server.server_close()
        evidence.with_suffix('.http.json').write_text(json.dumps(requests, indent=2) + '\n')
        evidence.write_text(json.dumps(result, indent=2) + '\n')
    return result


class Jsonl:
    def __init__(self, binary, root, env):
        self.process = subprocess.Popen([str(binary), '--mode', 'headless', '--session', str(root / 'sessions/initial.jsonl')],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1, cwd=root / 'initial', env=env, start_new_session=True)
        self.incoming = queue.Queue()
        self.rows = []
        self.responses = {}
        self.errors = []
        self.reader_error = None
        def read():
            try:
                for line in self.process.stdout:
                    row = json.loads(line)
                    self.rows.append(row)
                    self.incoming.put(row)
            except BaseException as error:
                self.reader_error = repr(error)
            finally:
                self.incoming.put(None)
        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()
        self.error_reader = threading.Thread(
            target=lambda: self.errors.extend(self.process.stderr.readlines()), daemon=True)
        self.error_reader.start()

    def kill(self):
        # All children here belong to this fixture's freshly created group.
        try:
            os.killpg(self.process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.process.wait(timeout=5)
        self.reader.join(timeout=2)
        self.error_reader.join(timeout=2)

    def send(self, id, **payload):
        self.process.stdin.write(json.dumps(dict(schema_version=2, id=id, **payload)) + '\n')
        self.process.stdin.flush()

    def response(self, id):
        if id in self.responses:
            return self.responses.pop(id)
        deadline = time.monotonic() + 15
        while True:
            remaining = deadline - time.monotonic()
            assert remaining > 0, ('response deadline', id, self.rows)
            row = self.incoming.get(timeout=remaining)
            assert row is not None, ('headless closed before response', id,
                                     self.rows, self.errors, self.reader_error)
            if row.get('type') == 'response':
                if row.get('id') == id:
                    return row
                self.responses[row.get('id')] = row

    def command(self, id, text):
        self.send(id, type='command', text=text)
        return self.response(id)

    def close(self):
        if self.process.poll() is None:
            self.send('fixture-stop', type='shutdown')
            assert self.response('fixture-stop')['success']
            self.process.stdin.close()
            self.process.wait(timeout=10)
        assert self.process.returncode == 0


def after_smoke(binary, evidence):
    from tui_project_workspace_smoke import screen_text
    requests, terminals, wires = [], [], []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            prompt = next((row.get('content', '') for row in reversed(body.get('input', [])) if row.get('role') == 'user'), '')
            events = []
            if 'NEW_TOOL' in str(prompt) and not any(row.get('type') == 'function_call_output' for row in body.get('input', [])):
                events.append({'type':'response.output_item.done','item':{'type':'function_call', 'call_id':'new-read', 'name':'read_file', 'arguments':json.dumps({'path':'same-cwd.txt'})}})
            else:
                events.append({'type':'response.output_text.delta', 'delta':'REPLY_NEW' if 'NEW_' in str(prompt) else 'REPLY_OLD'})
            events.append({'type':'response.completed','response':{'id':'fixture','model':body['model'],'status':'completed'}})
            raw = ''.join('data: ' + json.dumps(row) + '\n\n' for row in events).encode()
            self.send_response(200)
            self.send_header('Content-Type','text/event-stream')
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
    evidence = evidence.resolve()
    root = evidence.with_suffix('.fixture') / '工作 space'
    config = root / 'fixture'
    for path in (root / 'initial', root / 'sessions', config): path.mkdir(parents=True)
    (root / 'initial/same-cwd.txt').write_text('SAME_CANONICAL_PROJECT')
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    (config / 'config.toml').write_text(
        f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
        'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n'
        '[[model_overrides]]\nprovider="openai"\nid="new-reasoner"\nversion="fixture-1"\n'
        'context_window=64000\nmax_output_tokens=8192\ntext=true\nimages=true\nfiles=true\ntools=true\nstreaming=true\nreasoning_levels=["low","high"]\n')
    (config / 'auth.json').write_text('{"OPENAI_API_KEY":"session-new-fixture"}\n')
    (config / 'auth.json').chmod(0o600)
    env = {k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))
           and k.lower() not in ('http_proxy','https_proxy','all_proxy','no_proxy')}
    env.update(ZENPI_HOME=str(config), TERM='xterm-256color')
    result = {'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(), 'checks':{}}
    def saved(): return json.loads((config / 'project-tabs.json').read_text())
    def active():
        value = saved()
        return next(row for row in value['project_state'] if row['name'] == value['projects'][value['active']])
    def session_path(): return Path(active()['metadata']['session_path'])
    def session_id(): return saved()['session_cursors'][active()['name']]['session_id']
    def turns(path): return [row for row in map(json.loads,path.read_text().splitlines()) if row['kind'] == 'turn']
    def terminal():
        t = Terminal(binary, root, env); terminals.append(t)
        t.wait(lambda: len(screen_text(bytes(t.output)).splitlines()) > 1 and b'Ready' in screen_text(bytes(t.output)).splitlines()[1])
        return t
    def wire():
        w = Jsonl(binary, root, env); wires.append(w); return w
    try:
        t = terminal()
        t.command('OLD_CONTEXT_MARKER', b'REPLY_OLD')
        t.wait(lambda: len(turns(session_path())) == 2)
        t.command('/pane collapse resources')
        t.wait(lambda: 'resources' in active()['layout']['collapsed'])
        old_path, old_id, original = session_path(), session_id(), session_path().read_bytes()
        project, layout, tabs = active()['name'], active()['layout'], saved()['projects']
        count = len(requests)
        t.type_text('/new')
        t.expect(b'start a fresh conversation')
        t.deliberate_enter()
        t.wait(lambda: session_id() != old_id and t.draft() == '')
        new_path, new_id = session_path(), session_id()
        assert active()['name'] == project and active()['layout'] == layout and saved()['projects'] == tabs
        assert not turns(new_path) and len(requests) == count
        assert old_path.read_bytes().startswith(original)
        assert all('OLD_CONTEXT_MARKER' not in str(row) for row in active()['messages'])
        result['checks']['actual_tui_menu_new_same_project_empty_history_zero_http'] = True
        t.command('NEW_TOOL', b'REPLY_NEW')
        t.wait(lambda: len(requests) == count + 2)
        assert all('OLD_CONTEXT_MARKER' not in json.dumps(row) and 'REPLY_OLD' not in json.dumps(row) for row in requests[count:])
        assert 'SAME_CANONICAL_PROJECT' in json.dumps(requests[-1])
        result['checks']['next_actual_http_has_only_new_context_and_tool_uses_same_cwd'] = True
        t.close(preserve_draft=True)
        w = wire()
        w.send('restored', type='status')
        assert w.response('restored')['project']['session_id'] == new_id
        assert w.command('old-open', '/session open ' + json.dumps(str(old_path), ensure_ascii=False))['project']['session_id'] == old_id
        w.send('old-status', type='status')
        assert w.response('old-status')['data']['session']['turn_count'] == 2
        assert old_path.read_bytes().startswith(original)
        assert w.command('new-open', '/session open ' + json.dumps(str(new_path), ensure_ascii=False))['project']['session_id'] == new_id
        w.close()
        result['checks']['independent_jsonl_restore_old_history_and_open_new_again'] = True
        t = terminal()
        t.type_text('/model new-reasoner'); t.write(b'\t'); t.deliberate_enter(); t.expect(b'model selected: new-reasoner')
        t.type_text('/reasoning high'); t.write(b'\t'); t.deliberate_enter(); t.expect(b'reasoning selected: high')
        t.command('/persona ESFP')
        t.wait(lambda: t.draft() == '')
        draft = '保存 Unicode draft\n' + ''.join(f'折叠 row {i}\n' for i in range(140))
        t.write(b'\x1b[200~' + draft.encode() + b'\x1b[201~')
        t.wait(lambda: t.draft() != '' and active()['draft'].get('paste_folds'))
        draft_state, layout = active()['draft'], active()['layout']
        t.close(preserve_draft=True)
        w = wire()
        count = len(requests)
        response = w.command('durable-new', '/new')
        assert response['success'], response
        created = Path(response['data']['new_session_path'])
        assert len(requests) == count and not turns(created)
        assert w.command('durable-new','/new') == response
        assert w.command('durable-new','/status')['code'] == 'request_id_conflict'
        files = sorted(str(p) for p in (root/'sessions').glob('new-*.jsonl'))
        w.close()
        w = wire()
        assert w.command('durable-new','/new') == response
        assert sorted(str(p) for p in (root/'sessions').glob('new-*.jsonl')) == files
        w.close()
        result['checks']['same_id_and_independent_restart_replay_no_extra_journal_or_http'] = True
        t = terminal()
        t.wait(lambda: session_id() == response['data']['new_session_id'])
        for key in ('input','cursor','paste_folds','next_paste_id'):
            assert active()['draft'][key] == draft_state[key], (key, active()['draft'], draft_state)
        assert active()['layout'] == layout
        assert not active()['messages'] and len(requests) == count
        events = [row['event'] for row in map(json.loads,created.read_text().splitlines()) if row['kind'] == 'event']
        assert any(e.get('type') == 'model_selected' and e['model'] == 'new-reasoner' and e['reasoning_effort'] == 'high' for e in events)
        assert any(e.get('type') == 'persona_selected' and e['persona'] == 'ESFP' for e in events)
        result['checks']['saved_unicode_fold_draft_layout_preferences_restore_without_autosubmit'] = True
        t.clear_input()
        t.command('NEW_PREFERENCES', b'REPLY_NEW')
        t.wait(lambda: len(requests) == count + 1)
        assert requests[-1]['model'] == 'new-reasoner' and requests[-1]['reasoning']['effort'] == 'high'
        result['checks']['restored_model_effort_used_by_actual_http'] = True
        t.close(preserve_draft=True)

        gate = config / 'extensions/gate'
        result['fixture_executable_setup_ms'] = install_gate(gate, evidence)
        t = terminal()
        stable_id, stable_path = session_id(), session_path()
        stable_layout, stable_tabs = active()['layout'], saved()['projects']
        checkpoint = root / 'sessions/project-workspace.json'
        stable_checkpoint = checkpoint.read_bytes()
        count = len(requests)
        journals = set((root / 'sessions').glob('new-*.jsonl'))
        (gate / 'block').write_text('')
        t.type_text('/new')
        t.deliberate_enter()
        t.wait(lambda: (gate / 'entered').exists(), timeout=2)
        assert checkpoint.read_bytes() == stable_checkpoint
        t.write(b'\x03')
        t.expect(b'Interrupted')
        t.wait(lambda: t.draft() == '/new')
        assert session_id() == stable_id and session_path() == stable_path
        assert checkpoint.read_bytes() == stable_checkpoint
        assert active()['layout'] == stable_layout and saved()['projects'] == stable_tabs
        assert set((root / 'sessions').glob('new-*.jsonl')) == journals
        assert len(requests) == count
        (gate / 'block').unlink()
        (gate / 'release').write_text('')
        t.clear_input()
        t.command('NEW_AFTER_TUI_CANCEL', b'REPLY_NEW')
        t.wait(lambda: len(requests) == count + 1 and t.draft() == '')
        assert session_id() == stable_id
        result['checks']['actual_tui_ctrl_c_before_commit_restores_command_owner_layout_and_next_http'] = True

        # A second real JSONL owner commits while the TUI's prepared candidate
        # is waiting. The first host must reject its stale checkpoint proposal.
        for name in ('entered', 'release'):
            (gate / name).unlink(missing_ok=True)
        (gate / 'block').write_text('')
        journals = set((root / 'sessions').glob('new-*.jsonl'))
        count = len(requests)
        t.type_text('/new')
        t.deliberate_enter()
        t.wait(lambda: (gate / 'entered').exists(), timeout=2)
        candidate = set((root / 'sessions').glob('new-*.jsonl')) - journals
        assert len(candidate) == 1
        # The already running extension waits on release; newly initialized
        # processes can proceed after removing block.
        (gate / 'block').unlink()
        w = wire()
        competing = w.command('second-writer-new', '/new')
        assert competing['success'], competing
        competing_checkpoint = checkpoint.read_bytes()
        assert competing['data']['old_session_id'] == stable_id
        assert len(requests) == count
        w.close()
        (gate / 'release').write_text('')
        t.expect(b'checkpoint changed in another host')
        t.wait(lambda: t.draft() == '/new')
        assert all(not path.exists() for path in candidate)
        assert checkpoint.read_bytes() == competing_checkpoint
        assert session_id() == stable_id and active()['layout'] == stable_layout
        t.clear_input()
        t.command('NEW_AFTER_CONCURRENT_COMMIT', b'REPLY_NEW')
        t.wait(lambda: len(requests) == count + 1 and t.draft() == '')
        assert session_id() == stable_id
        t.close(preserve_draft=True)
        assert checkpoint.read_bytes() == competing_checkpoint
        w = wire()
        assert w.command('second-writer-new', '/new') == competing
        w.close()
        result['checks']['actual_second_headless_writer_wins_cas_first_tui_owner_remains_usable'] = True
        result.update(status='passed', tui_processes=len(terminals), jsonl_processes=len(wires), request_count=len(requests))
    except BaseException as error:
        result.update(status='failed', error=repr(error))
        raise
    finally:
        gate = config / 'extensions/gate'
        if gate.is_dir():
            (gate / 'block').unlink(missing_ok=True)
            (gate / 'release').write_text('')
        for t in terminals:
            t.cleanup()
        for w in wires:
            if w.process.poll() is None: w.kill()
        server.shutdown(); server.server_close()
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.http.json').write_text(json.dumps(requests,ensure_ascii=False,indent=2)+'\n')
        evidence.with_suffix('.jsonl.json').write_text(json.dumps([w.rows for w in wires],ensure_ascii=False,indent=2)+'\n')
        evidence.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    return result


def install_gate(gate, evidence):
    gate.mkdir(parents=True, exist_ok=True)
    (gate / 'plugin.c').write_text(r'''
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <unistd.h>
int main(void) {
char line[65536];
if (!fgets(line, sizeof(line), stdin)) return 2;
int initialize = strstr(line, "\"method\":\"initialize\"") != NULL;
char *id = strstr(line, "\"id\":");
char *cap = strstr(line, "\"capability\":{");
if (!id || !cap) return 3;
unsigned long request = strtoul(id + 5, NULL, 10);
cap += 13;
char *end = strchr(cap, '}');
if (!end) return 4;
end[1] = 0;
if (initialize && access("block", F_OK) == 0) {
    FILE *marker = fopen("entered", "w");
    if (!marker) return 5;
    fprintf(marker, "%ld", (long)getpid()); fclose(marker);
    while (access("release", F_OK) != 0) usleep(2000);
}
printf("{\"jsonrpc\":\"2.0\",\"id\":%lu,\"capability\":%s,\"result\":%s}\n", request, cap,
    initialize ? "{\"api_version\":2,\"hooks\":[\"session_start\"],\"tools\":[]}" : "{\"action\":\"continue\"}");
return 0;
}
''')
    compiled = subprocess.run(['cc', '-O0', '-o', str(gate / 'plugin'), str(gate / 'plugin.c')],
                              capture_output=True, text=True, timeout=30)
    evidence.with_suffix('.cc.log').write_text(compiled.stdout + compiled.stderr)
    assert compiled.returncode == 0
    cold = time.monotonic()
    warm = subprocess.run([str(gate / 'plugin')], stdin=subprocess.DEVNULL,
                          capture_output=True, timeout=10)
    elapsed_ms = (time.monotonic() - cold) * 1000
    assert warm.returncode == 2
    (gate / 'extension.toml').write_text(
        "name='gate'\nversion='1.0.0'\napi_version=2\nexecutable='plugin'\n"
        "hook_timeout_ms=1000\nhooks=['session_start']\n")
    return elapsed_ms


def recovery_smoke(binary, evidence):
    """Kill actual CLI process groups on either side of the owner checkpoint.

    The extension gate only delays real initialization. No production commit,
    response, journal or filesystem operation is mocked.
    """
    evidence = evidence.resolve()
    root = evidence.with_suffix('.fixture') / '恢复 project'
    config = root / 'fixture'
    gate = config / 'extensions/gate'
    for path in (root / 'initial', root / 'sessions', gate):
        path.mkdir(parents=True)
    requests, wires = [], []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            events = [{'type': 'response.output_text.delta', 'delta': 'RECOVERY_HTTP_OK'},
                      {'type': 'response.completed', 'response': {
                          'id': 'recovery', 'model': body['model'], 'status': 'completed'}}]
            raw = ''.join('data: ' + json.dumps(row) + '\n\n' for row in events).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    (config / 'config.toml').write_text(
        f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
        'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
    (config / 'auth.json').write_text('{"OPENAI_API_KEY":"recovery-fixture"}\n')
    (config / 'auth.json').chmod(0o600)
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('ZENPI_', 'OPENAI_'))
           and k.lower() not in ('http_proxy', 'https_proxy', 'all_proxy', 'no_proxy')}
    env.update(ZENPI_HOME=str(config), TERM='xterm-256color')
    result = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'checks': {}, 'scope': 'macOS/Unix actual CLI process recovery; not full ZS1-132 acceptance'}
    checkpoint = root / 'sessions/project-workspace.json'
    def checkpoint_bytes(): return checkpoint.read_bytes() if checkpoint.exists() else None
    def wait(predicate, seconds=5):
        deadline = time.monotonic() + seconds
        while not predicate():
            assert time.monotonic() < deadline, 'condition deadline'
            time.sleep(.002)
    def wire():
        w = Jsonl(binary, root, env)
        wires.append(w)
        return w
    def status(w, id):
        w.send(id, type='status')
        response = w.response(id)
        assert response['success'], response
        return response
    def prompt(w, id):
        count = len(requests)
        w.send(id, type='prompt', text=id)
        response = w.response(id)
        assert response['success'] and len(requests) == count + 1, response
        return response
    def block(w, id):
        for name in ('entered', 'release'):
            (gate / name).unlink(missing_ok=True)
        (gate / 'block').write_text('')
        existing = set((root / 'sessions').glob('new-*.jsonl'))
        w.send(id, type='command', text='/new')
        wait(lambda: (gate / 'entered').exists(), seconds=2)
        fresh = set((root / 'sessions').glob('new-*.jsonl')) - existing
        assert len(fresh) == 1, fresh
        return fresh.pop()
    def unblock():
        (gate / 'block').unlink(missing_ok=True)
        (gate / 'release').write_text('')
    try:
        result['fixture_executable_setup_ms'] = install_gate(gate, evidence)
        w = wire()
        old = prompt(w, 'BEFORE_CRASH_SEED')
        old_id = old['project']['session_id']
        original_path = root / 'sessions/initial.jsonl'
        original = original_path.read_bytes()
        before = checkpoint_bytes()
        count = len(requests)
        orphan = block(w, 'precommit-crash')
        assert checkpoint_bytes() == before
        assert not any(row.get('event', {}).get('type') == 'session_new_transition'
                       for row in map(json.loads, orphan.read_text().splitlines()))
        w.kill()
        assert w.process.returncode == -signal.SIGKILL
        unblock()
        assert checkpoint_bytes() == before and len(requests) == count
        w = wire()
        assert status(w, 'precommit-recovered')['project']['session_id'] == old_id
        assert prompt(w, 'AFTER_PRECOMMIT_CRASH')['project']['session_id'] == old_id
        assert original_path.read_bytes().startswith(original)
        result['checks']['actual_precommit_sigkill_restores_old_owner_and_next_http'] = True
        result['uncommitted_crash_candidate'] = str(orphan)

        # Do not consume a response: the checkpoint itself is the observable
        # commit point. Kill the independent process as soon as it changes.
        before = checkpoint_bytes()
        count = len(requests)
        w.send('postcommit-crash', type='command', text='/new')
        wait(lambda: checkpoint_bytes() != before)
        committed = checkpoint_bytes()
        mapping = json.loads(committed)
        new_path = Path(mapping['sessions'][old['project']['project_id']])
        new_id = json.loads(new_path.read_text().splitlines()[0])['session_id']
        w.kill()
        assert w.process.returncode == -signal.SIGKILL and len(requests) == count
        files = sorted(str(path) for path in (root / 'sessions').glob('new-*.jsonl'))
        w = wire()
        assert status(w, 'postcommit-recovered')['project']['session_id'] == new_id
        replay = w.command('postcommit-crash', '/new')
        assert replay['success'] and replay['data']['new_session_id'] == new_id, replay
        assert w.command('postcommit-crash', '/new') == replay
        assert w.command('postcommit-crash', '/status')['code'] == 'request_id_conflict'
        assert sorted(str(path) for path in (root / 'sessions').glob('new-*.jsonl')) == files
        assert len(requests) == count and checkpoint_bytes() == committed
        assert prompt(w, 'AFTER_POSTCOMMIT_CRASH')['project']['session_id'] == new_id
        result['checks']['actual_postcommit_sigkill_restores_new_owner_and_replays_once'] = True

        # Force a real post-commit replay-open error, retaining the transition
        # journal. Removing this fixture-owned obstruction permits recovery.
        count = len(requests)
        before = checkpoint_bytes()
        failed_path = block(w, 'replay-open-failure')
        obstruction = Path(str(failed_path) + '.reconnect')
        obstruction.mkdir()
        unblock()
        w.process.wait(timeout=10)
        w.reader.join(timeout=2)
        w.error_reader.join(timeout=2)
        assert w.process.returncode != 0
        assert 'created and committed' in ''.join(w.errors), w.errors
        assert checkpoint_bytes() != before and obstruction.is_dir()
        assert not any(row.get('id') == 'replay-open-failure' and row.get('success') is False
                       for row in w.rows), w.rows
        transition = next(row['event'] for row in map(json.loads, failed_path.read_text().splitlines())
                          if row.get('event', {}).get('type') == 'session_new_transition')
        assert transition['request_id'] == 'replay-open-failure'
        assert len(requests) == count
        obstruction.rmdir()
        files = sorted(str(path) for path in (root / 'sessions').glob('new-*.jsonl'))
        w = wire()
        recovered = w.command('replay-open-failure', '/new')
        assert recovered['success'] and recovered['data'] == transition['data'], recovered
        assert w.command('replay-open-failure', '/new') == recovered
        assert len(requests) == count
        assert sorted(str(path) for path in (root / 'sessions').glob('new-*.jsonl')) == files
        assert prompt(w, 'AFTER_REPLAY_FAILURE')['project']['session_id'] == recovered['data']['new_session_id']
        w.close()
        result['checks']['actual_replay_open_failure_reports_commit_and_recovers_same_receipt'] = True
        result.update(status='passed', jsonl_processes=len(wires), request_count=len(requests))
    except BaseException as error:
        result.update(status='failed', error=repr(error))
        raise
    finally:
        unblock()
        for w in wires:
            if w.process.poll() is None:
                w.kill()
        server.shutdown()
        server.server_close()
        evidence.with_suffix('.http.json').write_text(json.dumps(requests, ensure_ascii=False, indent=2) + '\n')
        evidence.with_suffix('.jsonl.json').write_text(json.dumps(
            [{'pid': w.process.pid, 'returncode': w.process.returncode, 'rows': w.rows,
              'stderr': w.errors, 'reader_error': w.reader_error} for w in wires],
            ensure_ascii=False, indent=2) + '\n')
        evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--evidence', type=Path, default=Path('.ops/session-new-result.json'))
    parser.add_argument('--before-only', action='store_true')
    parser.add_argument('--recovery-only', action='store_true')
    args = parser.parse_args()
    parser.error('--before-only and --recovery-only are mutually exclusive') if args.before_only and args.recovery_only else None
    operation = before_smoke if args.before_only else recovery_smoke if args.recovery_only else after_smoke
    print(json.dumps(operation(args.binary.resolve(), args.evidence), indent=2))


if __name__ == '__main__':
    main()

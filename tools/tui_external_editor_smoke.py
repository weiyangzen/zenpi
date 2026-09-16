#!/usr/bin/env python3
"""Actual Unix PTY editor handoff evidence; each run binds a fixed release hash."""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def digest(data):
    return hashlib.sha256(data).hexdigest()


def start_terminal(binary, root, env, terminals):
    from tui_composer_smoke import Terminal
    terminal = Terminal.__new__(Terminal)
    terminals.append(terminal)
    terminal.__init__(binary, root, env)
    return terminal


def cleanup_fixture_children(terminals, children):
    """Only process groups recorded by this fixture or foreground on its own PTYs."""
    import signal
    groups = set()
    for terminal in terminals:
        if terminal.pid and terminal.fd is not None:
            try:
                group = os.tcgetpgrp(terminal.fd)
                if group > 1 and group != terminal.pid and group != os.getpgrp():
                    groups.add(group)
            except OSError:
                pass
    for row in children:
        if row['event'] not in ('started', 'descendant'):
            continue
        try:
            if row['pgid'] > 1 and os.getpgid(row['pid']) == row['pgid'] and row['pgid'] != os.getpgrp():
                groups.add(row['pgid'])
        except ProcessLookupError:
            pass
    for group in groups:
        try:
            os.killpg(group, signal.SIGTERM)
        except ProcessLookupError:
            pass
    if groups:
        time.sleep(.25)
    for group in groups:
        try:
            os.killpg(group, signal.SIGKILL)
        except ProcessLookupError:
            pass


def editor_child():
    """An actual foreground process, with an explicit save-and-close interaction."""
    import termios
    record, plan_path = map(Path, sys.argv[2:4])
    path = Path(sys.argv[-1])
    plan = json.loads(plan_path.read_text())
    seed = path.read_bytes()
    fd = os.open(record, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)

    def log(event, **values):
        os.write(fd, (json.dumps(dict(event=event, **values), ensure_ascii=False) + '\n').encode())

    import signal
    action = plan.get('action', 'save')
    if action == 'ignore-term':
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    attrs = termios.tcgetattr(0)
    log('started', pid=os.getpid(), pgid=os.getpgrp(), foreground=os.tcgetpgrp(0),
        cwd=os.getcwd(), argv=sys.argv[1:], tty=[os.isatty(i) for i in range(3)],
        canonical=bool(attrs[3] & termios.ICANON), echo=bool(attrs[3] & termios.ECHO),
        blocking=os.get_blocking(0), environment_keys=sorted(os.environ),
        path=str(path), directory_mode=path.parent.stat().st_mode & 0o777,
        mode=path.stat().st_mode & 0o777, seed_hex=seed.hex(), seed_sha256=digest(seed))
    if plan.get('descendant'):
        child = os.fork()
        if child == 0:
            if plan.get('detached'):
                os.setsid()
            if plan.get('child_ignore_term'):
                signal.signal(signal.SIGTERM, signal.SIG_IGN)
            log('descendant', pid=os.getpid(), pgid=os.getpgrp(), detached=bool(plan.get('detached')))
            while True:
                time.sleep(1)
        log('fork', child=child)
    if plan.get('write_before_wait'):
        path.write_text('UNACCEPTED_PARTIAL_EDIT')
    print('ZENPI_EDITOR_READY', flush=True)
    line = sys.stdin.readline()
    log('input', text=line)
    if line != ('\x07save\n' if plan.get('second_shortcut') else 'save\n'):
        log('exit', code=23)
        os.close(fd)
        raise SystemExit(23)
    if action == 'nonzero':
        log('exit', code=7)
        raise SystemExit(7)
    if action == 'signal':
        os.kill(os.getpid(), signal.SIGTERM)
    output = plan.get('text', seed.decode()).encode()
    if action == 'invalid-utf8':
        output = b'\xff'
    if action == 'oversize':
        output = b'x' * (256 * 1024 + 1)
    if action == 'boundary':
        output = b'x' * (256 * 1024)
    if action in ('symlink', 'fifo', 'directory', 'missing'):
        path.unlink()
        if action == 'symlink':
            path.symlink_to(plan['outside'])
        elif action == 'fifo':
            os.mkfifo(path, 0o600)
        elif action == 'directory':
            path.mkdir()
    elif plan.get('atomic'):
        replacement = path.with_name('saved.md')
        out_fd = os.open(replacement, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(out_fd, 'wb') as stream:
            stream.write(output)
        replacement.replace(path)
    else:
        path.write_bytes(output)
    if action == 'permissions':
        path.chmod(0o644)
    if action == 'cleanup-entries':
        for index in range(257):
            (path.parent / f'backup-{index}').touch(mode=0o600)
    if action == 'cleanup-depth':
        (path.parent / '1/2/3/4/5').mkdir(parents=True)
    log('exit', code=0, output_sha256=digest(output), output_bytes=len(output))
    os.close(fd)


def smoke(binary, evidence, before=False):
    from tui_composer_smoke import Terminal
    from tui_user_shell_smoke import drain
    binary = binary.resolve()
    requests, terminals, children = [], [], []
    result = dict(started=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  binary=str(binary), binary_sha256=digest(binary.read_bytes()),
                  binary_bytes=binary.stat().st_size, platform=sys.platform,
                  mode='before' if before else 'roundtrip', checks={})

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            for kind, payload in [
                ('response.output_text.delta', {'delta': 'EDITOR_EXPLICIT_REPLY'}),
                ('response.completed', {'response': {'id': 'editor', 'model': 'gpt-4.1',
                                                     'status': 'completed'}}),
            ]:
                try:
                    self.wfile.write(('event: ' + kind + '\ndata: ' + json.dumps(
                        {'type': kind, **payload}) + '\n\n').encode())
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    return

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server_thread = threading.Thread(target=server.serve_forever)
    server_thread.start()
    Path('.ops').mkdir(exist_ok=True)
    fixture = tempfile.TemporaryDirectory(prefix='external-editor-pty-', dir=Path('.ops').resolve())
    root = Path(fixture.name)
    record = root / 'child.jsonl'
    try:
        config = root / 'fixture'
        for path in [root / 'initial', root / 'sessions', root / 'temp', config]:
            path.mkdir(mode=0o700)
        (config / 'config.toml').write_text(
            'backend="openai"\nmodel="gpt-4.1"\n'
            f'base_url="http://127.0.0.1:{server.server_port}/v1"\n'
            'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
        (config / 'auth.json').write_text(json.dumps({'OPENAI_API_KEY': 'external-editor-fixture'}))
        (config / 'auth.json').chmod(0o600)
        plan = root / 'plan.json'
        edited = '/exit\n!touch must-not-run\n界e\u0301👨‍👩‍👧‍👦  \n'
        plan.write_text(json.dumps(dict(text=edited, atomic=True)))
        program = root / '编辑器 with spaces'
        program.symlink_to(sys.executable)
        # Every path is generated by this fixture; quotes are explicit editor argv syntax.
        visual = ' '.join('"' + str(arg).replace('\\', '\\\\').replace('"', '\\"') + '"'
                          for arg in [program, Path(__file__).resolve(), '--editor-child', record,
                                      plan, '--wait', '', '$(touch sentinel)', ';'])
        env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
        env.update(ZENPI_HOME=str(config), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost', VISUAL=visual,
                   EDITOR='must-not-be-selected', TMPDIR=str(root / 'temp'),
                   ZENPI_EDITOR_TIMEOUT_SECONDS='15', OPENAI_API_KEY='must-not-reach-editor')
        t = start_terminal(binary, root, env, terminals)
        left, middle, right = 'prefix ' + '界' * 1001, '\n[Pasted #1, 1001 chars]\n', 'R' * 1001 + '\nTAIL  \n'
        seed = left + middle + right
        t.write(b'\x1b[200~KILL_EDITOR\x1b[201~\x15')
        t.wait(lambda: t.draft() == '')
        for part in [left, middle, right]:
            t.write(b'\x1b[200~' + part.encode() + b'\x1b[201~')
        t.wait(lambda: t.draft() == seed)
        def composer_snapshot():
            saved = json.loads(t.checkpoint.read_text())
            return next(row['draft'] for row in saved['project_state']
                        if row['name'] == saved['projects'][saved['active']])
        original = composer_snapshot()
        assert len(original['paste_folds']) == 2
        t.write(b'\x07')
        if before:
            until = time.monotonic() + 1.0
            while time.monotonic() < until:
                drain(t.fd, t.output, .05)
            assert not record.exists(), 'before unexpectedly launched the editor'
            assert t.draft() == seed and not requests
            result['checks']['ctrl_g_does_not_start_editor_on_fixed_before'] = True
        else:
            t.wait(lambda: record.exists())
            started = json.loads(record.read_text().splitlines()[0])
            assert all(started['tty']) and started['pgid'] == started['foreground']
            assert started['canonical'] and started['echo'] and started['blocking']
            assert started['cwd'] == str((root / 'initial').resolve())
            assert started['directory_mode'] == 0o700 and started['mode'] == 0o600
            assert bytes.fromhex(started['seed_hex']) == seed.encode()
            assert not any(k.startswith(('ZENPI_', 'OPENAI_')) for k in started['environment_keys'])
            assert started['argv'][-5:-1] == ['--wait', '', '$(touch sentinel)', ';']
            t.write(b'save\n\r')
            t.wait(lambda: t.draft() == edited)
            until = time.monotonic() + .4
            while time.monotonic() < until:
                drain(t.fd, t.output, .05)
            assert not requests and not (root / 'initial/must-not-run').exists()
            assert not (root / 'initial/sentinel').exists()
            assert not Path(started['path']).parent.exists()
            assert composer_snapshot()['paste_folds'] == []
            assert composer_snapshot()['next_paste_id'] >= original['next_paste_id']
            t.write(b'\x19')
            t.wait(lambda: t.draft() == edited + 'KILL_EDITOR')
            result['checks']['interactive_atomic_save_exact_text_no_implicit_submit'] = True
            result['checks']['multiple_folds_become_plain_and_ids_and_kill_buffer_survive'] = True
            explicit = 'EDITOR_EXPLICIT\n/exit\n!touch must-not-run\n界  \n'
            plan.write_text(json.dumps(dict(text=explicit, atomic=True)))
            count = len(record.read_text().splitlines())
            t.write(b'\x07')
            t.wait(lambda: len(record.read_text().splitlines()) > count)
            t.write(b'save\n\r')
            t.wait(lambda: t.draft() == explicit)
            assert not requests
            t.deliberate_enter()
            t.expect(b'EDITOR_EXPLICIT_REPLY')
            assert len(requests) == 1
            content = next(item for item in reversed(requests[0]['input']) if item.get('role') == 'user')['content']
            actual = content if isinstance(content, str) else ''.join(item.get('text', '') for item in content)
            assert actual == explicit, (actual, explicit)
            assert not (root/'initial/must-not-run').exists()
            result['checks']['later_deliberate_enter_sends_exact_full_text_once'] = True
        t.close(preserve_draft=True)
        result.update(status='passed', http_requests=len(requests), tui_processes=len(terminals))
    except Exception as error:
        result.update(status='failed', error=str(error))
        raise
    finally:
        if record.exists():
            children = [json.loads(line) for line in record.read_text().splitlines()]
        cleanup_fixture_children(terminals, children)
        for terminal in terminals:
            terminal.cleanup()
        server.shutdown()
        server_thread.join(timeout=3)
        server.server_close()
        result['ended'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.http.json').write_text(json.dumps(requests, ensure_ascii=False, indent=2) + '\n')
        evidence.with_suffix('.child.json').write_text(json.dumps(children, ensure_ascii=False, indent=2) + '\n')
        evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
        fixture.cleanup()
    return result


def failure_matrix(binary, evidence):
    """Real child file replacements, cancellation, no-op and input boundaries."""
    import signal
    from tui_composer_smoke import Terminal
    from tui_user_shell_smoke import drain
    binary = binary.resolve()
    calls, terminals, children = [], [], []
    result = dict(started=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  binary_sha256=digest(binary.read_bytes()), checks={})

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_POST(self):
            calls.append(self.rfile.read(int(self.headers['Content-Length'])).decode())
            self.send_response(503)
            self.end_headers()

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    fixture = tempfile.TemporaryDirectory(prefix='editor-negative-', dir=Path('.ops').resolve())
    root = Path(fixture.name)
    record = root / 'children.jsonl'
    try:
        config = root / 'fixture'
        for path in [root / 'initial', root / 'sessions', root / 'temp', config]:
            path.mkdir(mode=0o700)
        (config / 'config.toml').write_text('backend="openai"\nmodel="gpt-4.1"\n'
            f'base_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\n'
            'requires_openai_auth=false\nmax_retries=0\n')
        (config / 'auth.json').write_text('{"OPENAI_API_KEY":"editor-matrix-fixture"}')
        (config / 'auth.json').chmod(0o600)
        plan = root / 'plan.json'
        outside = root / 'outside'
        outside.write_text('OUTSIDE_UNCHANGED')
        visual = ' '.join('"' + str(arg) + '"' for arg in
                          [sys.executable, Path(__file__).resolve(), '--editor-child', record, plan])
        env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
        env.update(ZENPI_HOME=str(config), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost', VISUAL=visual,
                   TMPDIR=str(root/'temp'), ZENPI_EDITOR_TIMEOUT_SECONDS='2')
        t = start_terminal(binary, root, env, terminals)

        def records():
            return [json.loads(line) for line in record.read_text().splitlines()] if record.exists() else []

        def draft():
            value = json.loads((config/'project-tabs.json').read_text())
            active = value['projects'][value['active']]
            return next(row['draft'] for row in value['project_state'] if row['name'] == active)

        for case in ['noop', 'empty', 'nonzero', 'signal', 'ctrl-c', 'ctrl-z', 'timeout',
                     'ignore-term', 'symlink', 'fifo', 'directory', 'missing', 'permissions',
                     'invalid-utf8', 'oversize', 'boundary', 'cleanup-entries', 'cleanup-depth',
                     'prefetch-paste', 'prefetch-csi', 'prefetch-utf8']:
            t.clear_input()
            seed = 'SEED-' + '界' * 1001 + '\nend  \n'
            t.write(b'\x1b[200~' + seed.encode() + b'\x1b[201~')
            t.wait(lambda: t.draft() == seed)
            # A no-op must preserve the original fold and non-end cursor.
            if case == 'noop':
                t.write(b'\x1b[D')
                drain(t.fd, t.output, .3)
            original = draft()
            plan.write_text(json.dumps(dict(action=case, text='' if case == 'empty' else seed,
                                             outside=str(outside))))
            count = len(records())
            mark = len(t.output)
            suffix = {'prefetch-paste': b'\x1b[200~OLD', 'prefetch-csi': b'\x1b[',
                      'prefetch-utf8': b'\xe7'}.get(case, b'')
            t.write(b'\x07' + suffix)
            t.wait(lambda: len(records()) > count)
            started = records()[count]
            assert started['event'] == 'started'
            assert bytes.fromhex(started['seed_hex']) == seed.encode()
            assert all(started['tty']) and started['foreground'] == started['pgid']
            assert started['canonical'] and started['echo'] and started['blocking']
            if case == 'ctrl-c':
                t.write(b'\x03')
            elif case == 'ctrl-z':
                t.write(b'\x1a')
            elif case not in ('timeout', 'ignore-term'):
                t.write(b'save\n\r')
            t.wait(lambda: os.tcgetpgrp(t.fd) == t.pid and b'\x1b[?1049h' in t.output[mark:], timeout=8)
            drain(t.fd, t.output, .3)
            expected = '' if case == 'empty' else ('x' * (256 * 1024) if case == 'boundary' else seed)
            t.wait(lambda: t.draft() == expected)
            assert not calls and outside.read_text() == 'OUTSIDE_UNCHANGED'
            if case not in ('empty', 'boundary'):
                assert draft() == original, (case, draft(), original)
            private = Path(started['path']).parent
            if case.startswith('cleanup-'):
                from tui_project_workspace_smoke import screen_text
                assert 'cleanup failed' in screen_text(bytes(t.output)).decode().lower()
                assert private.exists()
                result['checks'][case + '_reported_retained_private_directory'] = str(private)
            else:
                assert not private.exists(), case
            try:
                os.kill(started['pid'], 0)
            except ProcessLookupError:
                pass
            else:
                raise AssertionError((case, 'editor leader still present'))
            if case.startswith('prefetch-'):
                t.write(b'x')
                t.wait(lambda: t.draft() == seed + 'x')
                assert not calls
            result['checks'][case] = True
        t.close(preserve_draft=True)
        saved = draft()['input']
        t = start_terminal(binary, root, env, terminals)
        t.wait(lambda: t.draft() == saved)
        assert not calls
        result['checks']['independent_restart_keeps_draft_without_resend'] = True
        t.close(preserve_draft=True)
        result.update(status='passed', tui_processes=len(terminals), http_requests=len(calls))
    except Exception as error:
        result.update(status='failed', error=str(error))
        raise
    finally:
        if record.exists():
            children = records()
        cleanup_fixture_children(terminals, children)
        for terminal in terminals:
            terminal.cleanup()
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()
        result['ended'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.child.json').write_text(json.dumps(children, ensure_ascii=False, indent=2)+'\n')
        evidence.with_suffix('.http.json').write_text(json.dumps(calls, indent=2)+'\n')
        evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
        fixture.cleanup()
    return result


def busy_matrix(binary, evidence):
    """Real model completion while editor owns TTY, and stale failed-input recovery."""
    from tui_composer_smoke import Terminal
    from tui_user_shell_smoke import drain
    binary = binary.resolve()
    calls, terminals = [], []
    gates = [dict(started=threading.Event(), release=threading.Event()) for _ in range(2)]
    result = dict(started=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  binary_sha256=digest(binary.read_bytes()), checks={})

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            index = len(calls)
            calls.append(body)
            if index >= len(gates):
                self.send_response(503)
                self.end_headers()
                return
            gate = gates[index]
            gate['started'].set()
            if not gate['release'].wait(15):
                self.send_response(503)
                self.end_headers()
                return
            if index == 1:
                self.send_response(503)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(b'{"error":{"message":"EDITOR_GATE_FAILURE"}}')
                return
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.end_headers()
            for kind, payload in [
                ('response.output_text.delta', {'delta': 'BACKGROUND_EDITOR_REPLY'}),
                ('response.completed', {'response': {'id': 'editor-busy', 'model': 'gpt-4.1',
                                                     'status': 'completed'}}),
            ]:
                try:
                    self.wfile.write(('event: '+kind+'\ndata: '+json.dumps(
                        {'type': kind, **payload})+'\n\n').encode())
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    return

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    fixture = tempfile.TemporaryDirectory(prefix='editor-busy-', dir=Path('.ops').resolve())
    root = Path(fixture.name)
    record = root/'children.jsonl'
    try:
        config = root/'fixture'
        for path in [root/'initial', root/'sessions', root/'temp', config]:
            path.mkdir(mode=0o700)
        (config/'config.toml').write_text('backend="openai"\nmodel="gpt-4.1"\n'
            f'base_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\n'
            'requires_openai_auth=false\nmax_retries=0\n')
        (config/'auth.json').write_text('{"OPENAI_API_KEY":"editor-busy-fixture"}')
        (config/'auth.json').chmod(0o600)
        plan = root/'plan.json'
        visual = ' '.join('"'+str(arg)+'"' for arg in
                          [sys.executable, Path(__file__).resolve(), '--editor-child', record, plan])
        env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
        env.update(ZENPI_HOME=str(config), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost', VISUAL=visual,
                   TMPDIR=str(root/'temp'), ZENPI_EDITOR_TIMEOUT_SECONDS='10')
        t = start_terminal(binary, root, env, terminals)

        def records():
            return [json.loads(line) for line in record.read_text().splitlines()] if record.exists() else []

        def saved():
            return json.loads((config/'project-tabs.json').read_text())

        for index in range(2):
            t.clear_input()
            if index == 1:
                t.write(b'\x1b[200~KEEP_KILL\x1b[201~\x15')
                t.wait(lambda: t.draft() == '')
            prompt = 'BUSY_EDITOR_GATE' if index == 0 else 'FAILED_EDITOR_GATE'
            t.write(prompt.encode())
            t.wait(lambda: t.draft() == prompt)
            t.deliberate_enter()
            t.wait(gates[index]['started'].is_set)
            t.wait(lambda: t.draft() == '')
            if index == 0:
                t.write(b'\x1b[200~draft while busy\x1b[201~')
                t.wait(lambda: t.draft() == 'draft while busy')
            plan.write_text(json.dumps(dict(text='EDITED_DURING_BUSY' if index == 0 else 'STALE_EXTERNAL')))
            count = len(records())
            t.write(b'\x07')
            t.wait(lambda: len(records()) > count)
            started = records()[count]
            t.wait(lambda: b'ZENPI_EDITOR_READY' in t.output)
            mark = len(t.output)
            assert bytes.fromhex(started['seed_hex']).decode() == ('draft while busy' if index == 0 else '')
            gates[index]['release'].set()
            if index == 0:
                t.wait(lambda: 'BACKGROUND_EDITOR_REPLY' in json.dumps(saved()))
            else:
                t.wait(lambda: t.draft() == prompt)
            assert b'\x1b' not in t.output[mark:], 'Zenpi drew while editor owned terminal'
            assert len(calls) == index + 1
            t.write(b'save\n\r')
            expected = 'EDITED_DURING_BUSY' if index == 0 else prompt
            t.wait(lambda: t.draft() == expected and os.tcgetpgrp(t.fd) == t.pid)
            drain(t.fd, t.output, .3)
            assert len(calls) == index + 1 and not Path(started['path']).parent.exists()
            if index == 0:
                result['checks']['busy_completion_checkpointed_without_drawing_or_reading_editor_keys'] = True
            else:
                t.write(b'\x19')
                t.wait(lambda: t.draft() == prompt + 'KEEP_KILL')
                result['checks']['failed_job_new_draft_rejects_stale_editor_and_preserves_kill_buffer'] = True
        t.close(preserve_draft=True)
        expected = t.draft()
        t = start_terminal(binary, root, env, terminals)
        t.wait(lambda: t.draft() == expected)
        assert len(calls) == 2
        t.close(preserve_draft=True)
        result.update(status='passed', tui_processes=len(terminals), http_requests=len(calls))
    except Exception as error:
        result.update(status='failed', error=str(error))
        raise
    finally:
        for gate in gates:
            gate['release'].set()
        children = records() if record.exists() else []
        cleanup_fixture_children(terminals, children)
        for terminal in terminals:
            terminal.cleanup()
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()
        result['ended'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.child.json').write_text(json.dumps(children, ensure_ascii=False, indent=2)+'\n')
        evidence.with_suffix('.http.json').write_text(json.dumps(calls, ensure_ascii=False, indent=2)+'\n')
        evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
        fixture.cleanup()
    return result


def lifecycle_matrix(binary, evidence):
    import fcntl
    import signal
    import struct
    import termios
    from tui_composer_smoke import Terminal
    from tui_user_shell_smoke import drain
    from tui_project_workspace_smoke import screen_text
    binary = binary.resolve()
    terminals, detached = [], []
    result = dict(started=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  binary_sha256=digest(binary.read_bytes()), checks={})
    fixture = tempfile.TemporaryDirectory(prefix='editor-lifecycle-', dir=Path('.ops').resolve())
    root = Path(fixture.name)
    record = root/'children.jsonl'
    try:
        config = root/'fixture'
        for path in [root/'initial', root/'sessions', root/'temp', config]:
            path.mkdir(mode=0o700)
        (config/'config.toml').write_text('backend="openai"\nmodel="gpt-4.1"\n'
            'base_url="http://127.0.0.1:9/v1"\nwire_api="responses"\n'
            'requires_openai_auth=false\nmax_retries=0\n')
        (config/'auth.json').write_text('{"OPENAI_API_KEY":"editor-lifecycle-fixture"}')
        (config/'auth.json').chmod(0o600)
        plan = root/'plan.json'
        visual = ' '.join('"'+str(arg)+'"' for arg in
                          [sys.executable, Path(__file__).resolve(), '--editor-child', record, plan])
        env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
        env.update(ZENPI_HOME=str(config), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost', VISUAL=visual,
                   TMPDIR=str(root/'temp'), ZENPI_EDITOR_TIMEOUT_SECONDS='10')
        def records():
            return [json.loads(line) for line in record.read_text().splitlines()] if record.exists() else []
        def gone(pid):
            try:
                os.kill(pid, 0)
                return False
            except ProcessLookupError:
                return True
        def new_tui():
            terminal = start_terminal(binary, root, env, terminals)
            return terminal
        t = new_tui()
        for case in ['resize', 'group-descendant', 'detached-boundary', 'parent-TERM', 'parent-HUP', 'parent-QUIT']:
            t.clear_input()
            seed = 'ORIGINAL_' + case
            t.write(b'\x1b[200~' + seed.encode() + b'\x1b[201~')
            t.wait(lambda: t.draft() == seed)
            plan.write_text(json.dumps(dict(text='SAVED_' + case, descendant=case != 'resize',
                detached=case == 'detached-boundary', child_ignore_term=case.startswith('parent-'),
                write_before_wait=case.startswith('parent-'))))
            count = len(records())
            mark = len(t.output)
            t.write(b'\x07')
            t.wait(lambda: len(records()) > count)
            started = records()[count]
            t.wait(lambda: b'ZENPI_EDITOR_READY' in t.output[mark:])
            descendants = []
            if case != 'resize':
                t.wait(lambda: any(row['event'] == 'descendant' for row in records()[count:]))
                descendants = [row for row in records()[count:] if row['event'] == 'descendant']
                assert len(descendants) == 1
                assert (descendants[0]['pgid'] != started['pgid']) == (case == 'detached-boundary')
            if case == 'resize':
                fcntl.ioctl(t.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 1, 0, 0))
                drain(t.fd, t.output, .15)
                fcntl.ioctl(t.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 140, 0, 0))
            if case.startswith('parent-'):
                sig = {'parent-TERM': signal.SIGTERM, 'parent-HUP': signal.SIGHUP,
                       'parent-QUIT': signal.SIGQUIT}[case]
                os.kill(t.pid, sig)
                deadline = time.monotonic() + 8
                while time.monotonic() < deadline:
                    drain(t.fd, t.output, .05)
                    pid, status = os.waitpid(t.pid, os.WNOHANG)
                    if pid:
                        t.pid = None
                        assert os.waitstatus_to_exitcode(status) == 0
                        attrs = termios.tcgetattr(t.fd)
                        assert attrs[3] & termios.ICANON and attrs[3] & termios.ECHO
                        os.close(t.fd)
                        t.fd = None
                        break
                else:
                    raise AssertionError('parent did not finish bounded shutdown')
                assert t.draft() == seed
                assert gone(started['pid']) and all(gone(row['pid']) for row in descendants)
                assert not Path(started['path']).parent.exists()
                t = new_tui()
                t.wait(lambda: t.draft() == seed)
                result['checks'][case + '_reaps_group_and_restarts_original_draft'] = True
            else:
                t.write(b'save\n\r')
                t.wait(lambda: t.draft() == 'SAVED_' + case and os.tcgetpgrp(t.fd) == t.pid)
                assert gone(started['pid']) and not Path(started['path']).parent.exists()
                if case == 'detached-boundary':
                    pid = descendants[0]['pid']
                    detached.append(pid)
                    assert not gone(pid), 'setsid process unexpectedly claimed by parent group cleanup'
                    os.kill(pid, signal.SIGTERM)
                    result['checks']['detached_service_outside_group_and_explicit_fixture_cleanup'] = True
                elif descendants:
                    t.wait(lambda: all(gone(row['pid']) for row in descendants))
                    result['checks']['ordinary_same_group_descendant_reclaimed_after_leader_exit'] = True
                else:
                    assert 'Prompt' in screen_text(bytes(t.output)).decode()
                    t.write(b'x')
                    t.wait(lambda: t.draft() == 'SAVED_resize' + 'x')
                    result['checks']['one_column_then_wide_resize_restores_bentobox_and_input'] = True
        t.close(preserve_draft=True)
        result.update(status='passed', tui_processes=len(terminals),
                      network_scope='No Enter/HTTP assertions in this lifecycle-only fixture; HTTP checked in other suites')
    except Exception as error:
        result.update(status='failed', error=str(error))
        raise
    finally:
        children = records() if record.exists() else []
        for pid in detached:
            if not gone(pid):
                os.kill(pid, signal.SIGKILL)
        cleanup_fixture_children(terminals, children)
        for terminal in terminals:
            terminal.cleanup()
        result['ended'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.child.json').write_text(json.dumps(children, ensure_ascii=False, indent=2)+'\n')
        evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
        fixture.cleanup()
    return result


def configuration_matrix(binary, evidence):
    import termios
    from tui_composer_smoke import Terminal
    from tui_user_shell_smoke import drain
    from tui_project_workspace_smoke import screen_text
    binary = binary.resolve()
    terminals = []
    result = dict(started=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  binary_sha256=digest(binary.read_bytes()), checks={})
    fixture = tempfile.TemporaryDirectory(prefix='editor-configuration-', dir=Path('.ops').resolve())
    root = Path(fixture.name)
    record = root/'children.jsonl'
    try:
        config = root/'fixture'
        for path in [root/'initial', root/'sessions', root/'temp', config]:
            path.mkdir(mode=0o700)
        (config/'config.toml').write_text('backend="openai"\nmodel="gpt-4.1"\n'
            'base_url="http://127.0.0.1:9/v1"\nwire_api="responses"\n'
            'requires_openai_auth=false\nmax_retries=0\n')
        (config/'auth.json').write_text('{"OPENAI_API_KEY":"editor-config-fixture"}')
        (config/'auth.json').chmod(0o600)
        (root/'initial/fixture.txt').write_text('local file')
        skill = root/'initial/.zenpi/skills/editor-source/SKILL.md'
        skill.parent.mkdir(parents=True)
        skill.write_text('---\nname: editor-source\ndescription: Editor source modal fixture\n---\nSOURCE_ONLY_FIXTURE\n')
        plan = root/'plan.json'
        plan.write_text(json.dumps(dict(text='CONFIGURATION_EDIT')))
        visual = ' '.join('"'+str(arg)+'"' for arg in
                          [sys.executable, Path(__file__).resolve(), '--editor-child', record, plan])
        base_env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
        base_env.update(ZENPI_HOME=str(config), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost', VISUAL=visual,
                        EDITOR=visual, TMPDIR=str(root/'temp'), ZENPI_EDITOR_TIMEOUT_SECONDS='10')
        noexec = root/'noexec'
        noexec.write_text('#!/bin/sh\nexit 0\n')
        noexec.chmod(0o600)
        badinterp = root/'bad-interpreter'
        badinterp.write_text('#!/nonexistent/zenpi-editor-interpreter\n')
        badinterp.chmod(0o700)
        (root/'initial/relative-editor').symlink_to(sys.executable)
        (root/'initial/via-path').symlink_to(sys.executable)
        invalid = [('missing', None), ('empty-visual', ''), ('nonutf8', os.fsdecode(b'\xff')),
                   ('quote', "'private-secret-unclosed"), ('raw-budget', 'x'*4097),
                   ('argument-budget', 'ed '+ 'x'*2049), ('argc-budget', 'ed '+ 'x '*32),
                   ('missing-program', '/nonexistent/zenpi-editor'), ('no-execute', str(noexec)),
                   ('spawn-error', str(badinterp)), ('environment-budget', visual), ('deadline', visual)]
        def records():
            return [json.loads(line) for line in record.read_text().splitlines()] if record.exists() else []
        def new_tui(env):
            t = start_terminal(binary, root, env, terminals)
            t.clear_input()
            return t
        for name, value in invalid:
            env = dict(base_env)
            if value is None:
                env.pop('VISUAL', None)
                env.pop('EDITOR', None)
            else:
                env['VISUAL'] = value
            if name == 'environment-budget':
                env['LC_TIME'] = 'x'*4097
            if name == 'deadline':
                env['ZENPI_EDITOR_TIMEOUT_SECONDS'] = '0'
            t = new_tui(env)
            t.write(b'\x1b[200~KEEP\x1b[201~')
            t.wait(lambda: t.draft() == 'KEEP')
            count = len(records())
            t.write(b'\x07')
            t.wait(lambda: any(label in screen_text(bytes(t.output)).decode() for label in
                ['External editor unavailable', 'Editor command', 'Editor program',
                 'Editor could not start', 'Editor environment', 'Editor timeout']))
            t.wait(lambda: os.tcgetpgrp(t.fd) == t.pid and not termios.tcgetattr(t.fd)[3] & termios.ICANON)
            assert len(records()) == count and t.draft() == 'KEEP'
            assert not list((root/'temp').iterdir()), name
            assert b'private-secret-unclosed' not in t.output
            t.write(b'x')
            t.wait(lambda: t.draft() == 'KEEPx')
            t.close(preserve_draft=True)
            result['checks'][name + '_keeps_draft_and_terminal'] = True
        for name in ['fallback', 'relative-program', 'captured-path', 'nonutf8-key']:
            env = dict(base_env)
            if name == 'fallback':
                env.pop('VISUAL')
            elif name == 'nonutf8-key':
                env[os.fsdecode(b'\xfe')] = 'unrelated variable'
            elif name == 'relative-program':
                env['VISUAL'] = visual.replace('"'+sys.executable+'"', '"./relative-editor"', 1)
            else:
                env['VISUAL'] = visual.replace('"'+sys.executable+'"', '"via-path"', 1)
                env['PATH'] = str(root/'initial') + os.pathsep + base_env.get('PATH', '')
            t = new_tui(env)
            count = len(records())
            t.write(b'\x07')
            t.wait(lambda: len(records()) > count)
            t.write(b'save\n')
            t.wait(lambda: t.draft() == 'CONFIGURATION_EDIT')
            t.close(preserve_draft=True)
            result['checks'][name] = True
        t = new_tui(base_env)
        for name, keys in [('history', b'\x12'), ('directory', b'\x14'), ('transcript', b'\x1bb')]:
            count = len(records())
            t.write(keys)
            drain(t.fd, t.output, .1)
            t.write(b'\x07')
            drain(t.fd, t.output, .3)
            assert len(records()) == count
            t.write(b'\x1b')
            drain(t.fd, t.output, .1)
            result['checks'][name + '_focus_blocks_editor'] = True
        for name, text in [('slash', '/mo'), ('model', '/model'), ('file', '/attach f')]:
            t.clear_input()
            t.type_text(text)
            t.wait(lambda: t.draft() == text)
            if name == 'file':
                t.expect(b'fixture.txt')
            count = len(records())
            t.write(b'\x07')
            drain(t.fd, t.output, .2)
            assert len(records()) == count
            t.write(b'\x1b')
            drain(t.fd, t.output, .1)
            t.write(b'\x07')
            t.wait(lambda: len(records()) > count)
            t.write(b'save\n')
            t.wait(lambda: t.draft() == 'CONFIGURATION_EDIT')
            result['checks'][name + '_popup_blocks_then_explicit_dismissal_allows_editor'] = True
        t.clear_input()
        t.type_text('/skill:editor-source')
        t.expect(b'F2 source')
        t.write(b'\x1bOQ')
        t.expect(b'Resource source')
        count = len(records())
        t.write(b'\x07')
        drain(t.fd, t.output, .3)
        assert len(records()) == count
        t.write(b'\x1b')
        drain(t.fd, t.output, .1)
        result['checks']['resource_source_inspector_focus_blocks_editor'] = True
        t.clear_input()
        plan.write_text(json.dumps(dict(text='SINGLE_SESSION', second_shortcut=True)))
        count = len(records())
        t.write(b'\x07')
        t.wait(lambda: len(records()) > count)
        t.write(b'\x07save\n')
        t.wait(lambda: t.draft() == 'SINGLE_SESSION')
        assert sum(row['event'] == 'started' for row in records()[count:]) == 1
        assert next(row for row in records()[count:] if row['event'] == 'input')['text'] == '\x07save\n'
        result['checks']['second_ctrl_g_reaches_only_current_editor'] = True
        t.clear_input()
        t.write(b'!printf approval-fixture')
        t.wait(lambda: t.draft() == '!printf approval-fixture')
        t.deliberate_enter()
        t.expect(b'Approval required')
        count = len(records())
        t.write(b'\x07')
        drain(t.fd, t.output, .3)
        assert len(records()) == count
        t.write(b'n\r')
        t.wait(lambda: 'Approval decision submitted: deny once' in screen_text(bytes(t.output)).decode())
        result['checks']['approval_focus_blocks_editor_and_requires_explicit_denial'] = True
        t.close(preserve_draft=True)
        # Seed a private, valid restart checkpoint with bounded history records.
        # No live state or product metadata is mutated by a test-only control path.
        checkpoint = config/'project-tabs.json'
        value = json.loads(checkpoint.read_text())
        active = value['projects'][value['active']]
        draft = next(row['draft'] for row in value['project_state'] if row['name'] == active)
        draft.update(input='NEAR_BUDGET', cursor=len('NEAR_BUDGET'), paste_folds=[], history=[])
        target = 4 * 1024 * 1024 - 16384
        encode = lambda: json.dumps(value, ensure_ascii=False, indent=2).encode()
        while len(encode()) < target:
            draft['history'].append('h' * min(250000, target - len(encode())))
        overflow = len(encode()) - target
        if overflow:
            draft['history'][-1] = draft['history'][-1][:-overflow]
        checkpoint.write_bytes(encode())
        t = start_terminal(binary, root, base_env, terminals)
        t.wait(lambda: t.draft() == 'NEAR_BUDGET')
        drain(t.fd, t.output, .8)
        assert checkpoint.stat().st_size > target - 4096
        before_hash = digest(checkpoint.read_bytes())
        before_bytes = checkpoint.stat().st_size
        count = len(records())
        mark = len(t.output)
        plan.write_text(json.dumps(dict(action='boundary')))
        t.write(b'\x07')
        t.wait(lambda: len(records()) > count)
        started = records()[count]
        t.write(b'save\n\r')
        t.wait(lambda: os.tcgetpgrp(t.fd) == t.pid and b'\x1b[?1049h' in t.output[mark:])
        t.wait(lambda: '4 MiB' in screen_text(bytes(t.output)).decode())
        assert t.draft() == 'NEAR_BUDGET'
        assert digest(checkpoint.read_bytes()) == before_hash
        assert not Path(started['path']).parent.exists()
        result['checks']['near_4mib_checkpoint_rejects_editor_atomically'] = dict(
            checkpoint_bytes=before_bytes, unchanged_sha256=before_hash, editor_bytes=256*1024)
        t.close(preserve_draft=True)
        result.update(status='passed', tui_processes=len(terminals))
    except Exception as error:
        result.update(status='failed', error=str(error))
        raise
    finally:
        children = records() if record.exists() else []
        cleanup_fixture_children(terminals, children)
        for terminal in terminals:
            terminal.cleanup()
        result['ended'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
        evidence.with_suffix('.child.json').write_text(json.dumps(children, ensure_ascii=False, indent=2)+'\n')
        evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
        fixture.cleanup()
    return result


def synchronous_host(binary, evidence, workspace, toolchain):
    """An independently linked caller of public run_with_state under a real PTY."""
    import fcntl
    import pty
    import signal
    import struct
    import subprocess
    import termios
    from tui_user_shell_smoke import drain
    workspace = workspace.resolve()
    root = evidence.resolve().with_suffix('.sync-build')
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    (root/'src').mkdir()
    (root/'initial').mkdir()
    (root/'temp').mkdir()
    (root/'src/main.rs').write_text(r'''
use std::{fs, path::PathBuf};
use zenpi::tui::{run_with_state, TuiConfig, TuiState};
fn main() {
    let destination = PathBuf::from(std::env::args_os().nth(1).unwrap());
    let mut state = TuiState::default();
    state.set_input("SYNC_SEED");
    run_with_state(TuiConfig::default(), state, move |text, state| {
        fs::write(&destination, text)?;
        state.set_status("SYNC_SUBMITTED");
        Ok::<(), std::io::Error>(())
    }).unwrap();
}
''')
    (root/'Cargo.toml').write_text('[package]\nname="editor-sync-probe"\nversion="0.0.0"\nedition="2024"\n'
        '[dependencies]\nzenpi={path='+json.dumps(str(workspace))+'}\n'
        '[patch.crates-io]\ncrossterm={path='+json.dumps(str(workspace/'vendor/crossterm'))+'}\n')
    # Seed the consumer with the repository's resolved versions. Offline alone
    # can otherwise select newer packages from the local registry cache.
    source_lock = (workspace/'Cargo.lock').read_bytes()
    (root/'Cargo.lock').write_bytes(source_lock)
    argv = ['cargo', '+'+toolchain, 'build', '--offline', '--manifest-path', str(root/'Cargo.toml')]
    result = dict(started=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  production_binary_sha256=digest(binary.read_bytes()), build_argv=argv,
                  build_cwd=str(workspace), source_lock_sha256=digest(source_lock), checks={})
    raw = bytearray()
    pid = fd = None
    status = None
    record, plan, submitted = root/'child.jsonl', root/'plan.json', root/'submitted.txt'
    try:
        with (root/'build.log').open('wb') as output:
            build = subprocess.run(argv, cwd=workspace, stdout=output, stderr=subprocess.STDOUT, timeout=240)
        result['build_exit'] = build.returncode
        result['consumer_lock_sha256'] = digest((root/'Cargo.lock').read_bytes())
        assert build.returncode == 0, (root/'build.log').read_text()[-2500:]
        probe = root/'target/debug/editor-sync-probe'
        result['probe_sha256'] = digest(probe.read_bytes())
        edited = 'SYNC_EDITED\n界  \n'
        plan.write_text(json.dumps(dict(text=edited, atomic=True)))
        visual = ' '.join('"'+str(arg)+'"' for arg in
                          [sys.executable, Path(__file__).resolve(), '--editor-child', record, plan])
        env = {k: v for k, v in os.environ.items() if not k.startswith(('ZENPI_', 'OPENAI_'))}
        env.update(VISUAL=visual, TMPDIR=str(root/'temp'), TERM='xterm-256color', NO_PROXY='127.0.0.1,localhost', no_proxy='127.0.0.1,localhost')
        pid, fd = pty.fork()
        if pid == 0:
            os.chdir(root/'initial')
            os.execve(str(probe), [str(probe), str(submitted)], env)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 140, 0, 0))
        def wait(predicate):
            deadline = time.monotonic()+12
            while time.monotonic() < deadline:
                drain(fd, raw, .05)
                if predicate():
                    return
            raise AssertionError(bytes(raw[-4000:]))
        wait(lambda: b'SYNC_SEED' in raw)
        os.write(fd, b'\x07')
        wait(record.exists)
        child = json.loads(record.read_text().splitlines()[0])
        assert bytes.fromhex(child['seed_hex']) == b'SYNC_SEED'
        assert all(child['tty']) and child['foreground'] == child['pgid']
        mark = len(raw)
        os.write(fd, b'save\n\r')
        wait(lambda: os.tcgetpgrp(fd) == pid and b'\x1b[?1049h' in raw[mark:])
        assert not submitted.exists()
        os.write(fd, b'\r')
        wait(submitted.exists)
        assert submitted.read_bytes() == edited.encode()
        result['checks']['public_sync_host_editor_roundtrip_then_explicit_submit_exact'] = True
        os.kill(pid, signal.SIGTERM)
        deadline = time.monotonic()+8
        while time.monotonic() < deadline:
            drain(fd, raw, .05)
            child_pid, child_status = os.waitpid(pid, os.WNOHANG)
            if child_pid:
                status = child_status
                break
        assert status is not None and os.waitstatus_to_exitcode(status) == 0
        result.update(status='passed', exit=0)
    except Exception as error:
        result.update(status='failed', error=str(error))
        raise
    finally:
        if fd is not None:
            os.close(fd)
        if pid and status is None:
            try:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
            except ProcessLookupError:
                pass
        children = [json.loads(line) for line in record.read_text().splitlines()] if record.exists() else []
        cleanup_fixture_children([], children)
        result['ended'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        evidence.with_suffix('.pty.log').write_bytes(raw)
        evidence.with_suffix('.child.json').write_text(json.dumps(children, ensure_ascii=False, indent=2)+'\n')
        evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=Path('target/release/zenpi'))
    parser.add_argument('--evidence', type=Path, default=Path('.ops/tui-external-editor.json'))
    parser.add_argument('--before', action='store_true')
    parser.add_argument('--roundtrip-only', action='store_true')
    parser.add_argument('--matrix', action='store_true')
    parser.add_argument('--busy', action='store_true')
    parser.add_argument('--lifecycle', action='store_true')
    parser.add_argument('--configuration', action='store_true')
    parser.add_argument('--sync', action='store_true')
    parser.add_argument('--workspace', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--toolchain', default=os.environ.get('RUSTUP_TOOLCHAIN', 'stable'))
    args = parser.parse_args()
    if args.sync:
        result = synchronous_host(args.binary, args.evidence, args.workspace, args.toolchain)
    elif args.configuration:
        result = configuration_matrix(args.binary, args.evidence)
    elif args.lifecycle:
        result = lifecycle_matrix(args.binary, args.evidence)
    elif args.busy:
        result = busy_matrix(args.binary, args.evidence)
    elif args.matrix:
        result = failure_matrix(args.binary, args.evidence)
    elif args.before or args.roundtrip_only:
        result = smoke(args.binary, args.evidence, args.before)
    else:
        args.evidence = args.evidence.resolve()
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        result = dict(status='running', started=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                      binary_sha256=digest(args.binary.read_bytes()),
                      helper_sha256=digest(Path(__file__).read_bytes()),
                      argv=sys.argv, cwd=str(Path.cwd()), suites={})
        suites = [('roundtrip', smoke), ('files-cancel-prefetch', failure_matrix),
                  ('busy', busy_matrix), ('lifecycle', lifecycle_matrix),
                  ('configuration', configuration_matrix), ('sync', synchronous_host)]
        try:
            for name, suite in suites:
                output = args.evidence.with_name(args.evidence.stem+'-'+name+'.json')
                extra = (args.workspace, args.toolchain) if name == 'sync' else ()
                try:
                    detail = suite(args.binary, output, *extra)
                    result['suites'][name] = dict(status=detail['status'], evidence=str(output),
                                                 sha256=digest(output.read_bytes()))
                except Exception as error:
                    result['suites'][name] = dict(status='failed', evidence=str(output), error=str(error))
            result['status'] = 'passed' if all(row['status'] == 'passed' for row in result['suites'].values()) else 'failed'
        finally:
            result['ended'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            args.evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
        if result['status'] != 'passed':
            raise AssertionError(result)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--editor-child':
        editor_child()
    else:
        main()

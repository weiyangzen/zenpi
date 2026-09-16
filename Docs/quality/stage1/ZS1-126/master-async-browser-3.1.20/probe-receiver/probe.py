#!/usr/bin/env python3
"""ZS1-126: prepare/check only by default; --mode run explicitly opens one PTY.

No provider prompts, HTTP server, FIFO, special session file or product hook.
A run needs a fresh output directory and the actual merged binary supplied by
its owner. macOS sandbox-exec denies network for the PTY child.
"""
from pathlib import Path
import argparse
import base64
import fcntl
import hashlib
import json
import os
import platform
import pty
import re
import select
import signal
import stat
import struct
import subprocess
import termios
import time
import traceback
from screen_adapter import screen_text

LIMIT = 64 * 1024 * 1024
STEPS = ('list', 'selection', 'search-failure', 'project')
MARKERS = {'A126', 'B126', 'C126', 'O126', 'D126', 'E126', 'G126'}


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')


def record_file(path, session_id, cwd, text, turns=1):
    """Produce ordinary v1 envelopes matching the frozen Rust decoder."""
    if path.exists():
        raise ValueError('fixture file already exists: ' + str(path))
    stamp = 1789171200000
    common = dict(schema_version=1, session_id=session_id,
                  timestamp_ms=stamp, timestamp='2026-09-12T00:00:00.000Z')
    rows = [dict(common, kind='session', seq=0, version=1,
                 created_at_ms=stamp, cwd=str(cwd))]
    for i in range(turns):
        rows.append(dict(common, kind='turn', seq=i+1,
                         turn=dict(id=session_id+'-turn-'+str(i), parent_id=None,
                                   role='user', content=text, created_at_ms=stamp)))
    encoded = [(json.dumps(row, separators=(',', ':'))+'\n').encode() for row in rows]
    assert all(len(line) <= 1024*1024 for line in encoded)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as f:
        for line in encoded:
            f.write(line)


def inventory(root, validate=False):
    rows = []
    for path in sorted(root.rglob('*.jsonl')):
        mode = path.lstat().st_mode
        if stat.S_ISDIR(mode):
            # The sole deliberate error is a NORMAL directory, never a FIFO.
            assert path.name == 'bad.jsonl', str(path)
            rows.append(dict(path=str(path.relative_to(root)), kind='directory'))
            continue
        assert stat.S_ISREG(mode), 'non-regular session fixture: '+str(path)
        if validate:
            items = [json.loads(line) for line in path.read_text().splitlines()]
            assert items[0]['kind'] == 'session' and items[0]['version'] == 1
            sid = items[0]['session_id']
            for i, item in enumerate(items):
                assert item['schema_version'] == 1 and item['seq'] == i
                assert item['session_id'] == sid
                if item['kind'] == 'turn':
                    assert item['turn']['role'] == 'user' and item['turn']['id']
        rows.append(dict(path=str(path.relative_to(root)), kind='file',
                         bytes=path.stat().st_size, sha256=digest(path)))
    total = sum(row.get('bytes', 0) for row in rows)
    assert total <= LIMIT, 'session fixture bytes exceed 64 MiB'
    return dict(total_session_bytes=total, limit=LIMIT, files=rows)


def prepare(root, pressure_mib):
    cwd = root/'initial'
    home = cwd/'g'
    local = cwd/'l'
    other = cwd/'other'
    for path in (home/'sessions', local, other/'s'):
        path.mkdir(parents=True, exist_ok=True)
    (home/'config.toml').write_text('backend="openai"\nmodel="fixture-126"\n'
        'base_url="http://127.0.0.1:9/v1"\nwire_api="responses"\n'
        'requires_openai_auth=false\nmax_retries=0\n')
    save(home/'auth.json', {'OPENAI_API_KEY': 'fixture-126-not-a-real-key'})
    (home/'auth.json').chmod(0o600)
    specs = [(local/'m-owner.jsonl', 'O126', cwd, 'owner body'),
             (local/'b.jsonl', 'B126', cwd, 'needle126'),
             (local/'c.jsonl', 'C126', cwd, 'needle126 race126'),
             (home/'sessions/G126.jsonl', 'G126', cwd, 'global only'),
             (other/'s/d.jsonl', 'D126', other, 'otherneedle126'),
             (other/'s/e.jsonl', 'E126', other, 'other unmatched')]
    for path, sid, base, text in specs:
        record_file(path, sid, base, text)
    # 4 * 240 KiB turns plus bounded envelopes per file, below one MiB/file.
    for i in range(pressure_mib):
        record_file(local/('p%02d.jsonl' % i), 'P%02d' % i, cwd,
                    'f' * (240 * 1024), turns=4)
    return dict(cwd=cwd, home=home, local=local, other=other,
                owner=local/'m-owner.jsonl', other_owner=other/'s/d.jsonl')


def fixture_env(paths):
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('ZENPI_', 'OPENAI_', 'ANTHROPIC_', 'GOOGLE_', 'GEMINI_'))}
    env.update(ZENPI_HOME=str(paths['home']), TERM='xterm-256color',
               ZENPI_API_KEY='fixture-126-not-a-real-key')
    assert env.get('HOME') == os.environ.get('HOME')
    assert env.get('CODEX_HOME') == os.environ.get('CODEX_HOME')
    return env


def checked_command(argv, paths, env, evidence, name):
    start = time.monotonic()
    result = {'argv': argv, 'cwd': str(paths['cwd']), 'timeout_seconds': 15}
    try:
        p = subprocess.run(argv, cwd=paths['cwd'], env=env, capture_output=True, timeout=15)
        result.update(exit_code=p.returncode, elapsed_seconds=time.monotonic()-start)
        (evidence/(name+'.stdout')).write_bytes(p.stdout)
        (evidence/(name+'.stderr')).write_bytes(p.stderr)
        assert p.returncode == 0, name+' exited '+str(p.returncode)
        return p.stdout
    except subprocess.TimeoutExpired as error:
        (evidence/(name+'.stdout')).write_bytes(error.stdout or b'')
        (evidence/(name+'.stderr')).write_bytes(error.stderr or b'')
        result.update(status='timeout', elapsed_seconds=time.monotonic()-start)
        raise
    finally:
        save(evidence/(name+'.command.json'), result)


def preflight(binary, paths, env, evidence):
    help_text = checked_command([str(binary), '--help'], paths, env, evidence, 'binary-help')
    for option in (b'--mode', b'--session', b'--backend', b'--model'):
        assert option in help_text, 'installed binary help lacks '+repr(option)
    data = checked_command([str(binary), 'session', 'inspect', str(paths['local']/'b.jsonl'), '--json'],
                           paths, env, evidence, 'fixture-inspect')
    view = json.loads(data)
    assert view['summary']['session_id'] == 'B126', view
    assert view['summary']['turn_count'] == 1 and not view['recovery_warnings'], view
    return dict(help_parameters=True, installed_decoder_accepts_fixture=True)


def sandbox_profile(binary, root):
    # Keep HOME/CODEX_HOME values, but deny user data outside this fixture.
    # Binary bytes are the only read exception outside the fixture itself.
    rules = ['(version 1)', '(allow default)', '(deny network*)']
    protected = {str(Path(os.environ[key]).resolve()) for key in ('HOME', 'CODEX_HOME')
                 if os.environ.get(key)}
    assert protected, 'run requires a known HOME boundary'
    for directory in sorted(protected):
        outside = '(require-all (subpath '+json.dumps(directory)+') (require-not (subpath '+json.dumps(str(root))+'))'
        rules.append('(deny file-read-data '+outside+' (require-not (literal '+json.dumps(str(binary))+'))))')
        rules.append('(deny file-write* '+outside+'))')
    return '\n'.join(rules)


class Terminal:
    """Adapted from the fully read Terminal helper; keeps slave attrs for restore evidence."""
    def __init__(self, binary, paths, env, evidence):
        self.root, self.paths, self.evidence = evidence/'fixture', paths, evidence
        self.output = bytearray()
        self.pid = self.fd = self.slave = None
        self.lifetime = None
        self.app_exit = evidence/'app-exit.json'
        self.status = None
        self.started = False
        self.checkpoint = paths['home']/'project-tabs.json'
        self.events = (evidence/'events.jsonl').open('x')
        self.raw = (evidence/'pty.raw').open('xb')
        self.serial = 0
        profile = sandbox_profile(binary, self.root)
        (evidence/'sandbox.sb').write_text(profile+'\n')
        self.argv = ['/usr/bin/sandbox-exec', '-f', str(evidence/'sandbox.sb'),
                     str(binary), '--mode', 'tui', '--backend', 'openai', '--model', 'fixture-126',
                     '--session', str(paths['owner'])]
        save(evidence/'launch.json', dict(argv=self.argv, cwd=str(paths['cwd']),
             HOME_preserved=True, CODEX_HOME_preserved=True, ZENPI_HOME=str(paths['home']),
             network='child sandbox denies network*', prompts='slash commands only',
             user_data='HOME/CODEX_HOME read-data denied outside fixture except binary; user writes denied outside fixture'))

    def event(self, kind, **values):
        self.events.write(json.dumps(dict(t=time.monotonic(), kind=kind, **values))+'\n')
        self.events.flush()

    def start(self):
        assert platform.system() == 'Darwin', 'run mode currently requires macOS sandbox-exec'
        assert Path('/usr/bin/sandbox-exec').is_file()
        self.fd, self.slave = pty.openpty()
        self.started = True
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 140, 0, 0))
        self.attrs_before = termios.tcgetattr(self.slave)
        lifetime_read, lifetime_write = os.pipe()
        pid = os.fork()
        if pid == 0:
            app = None
            try:
                os.close(lifetime_write)
                os.close(self.fd)
                os.setsid()
                fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)
                for target in (0, 1, 2):
                    os.dup2(self.slave, target)
                if self.slave > 2:
                    os.close(self.slave)
                os.chdir(self.paths['cwd'])
                # Keep a shell-like controlling-session owner alive while the
                # actual sandboxed application exits. macOS hangs up this PTY
                # if its session leader exits before the parent reads termios.
                stopping = [False]
                def stop(_signal, _frame):
                    stopping[0] = True
                signal.signal(signal.SIGTERM, stop)
                app = subprocess.Popen(self.argv, env=fixture_env(self.paths))
                save(self.evidence/'app-process.json', dict(pid=app.pid, supervisor=os.getpid()))
                sent_term = None
                while app.poll() is None:
                    if select.select([lifetime_read], [], [], .02)[0]:
                        os.read(lifetime_read, 1)
                        stopping[0] = True
                    if stopping[0] and sent_term is None:
                        app.terminate()
                        sent_term = time.monotonic()
                    if sent_term is not None and time.monotonic()-sent_term > 2:
                        app.kill()
                save(self.app_exit, dict(pid=app.pid, returncode=app.returncode))
                # The parent checks original slave attributes after app wait,
                # then releases us. EOF also releases us if the parent fails.
                while not stopping[0]:
                    if select.select([lifetime_read], [], [], .1)[0]:
                        os.read(lifetime_read, 1)
                        break
                os.close(lifetime_read)
                os._exit(app.returncode if app.returncode >= 0 else 128-app.returncode)
            except BaseException:
                traceback.print_exc()
                if app is not None and app.poll() is None:
                    app.kill()
                    app.wait(timeout=3)
                os._exit(127)
        os.close(lifetime_read)
        self.lifetime = lifetime_write
        self.pid = pid
        self.event('spawn', pid=pid)
        self.wait(lambda: self.checkpoint.exists() and b'Prompt' in self.screen())

    def pump(self, seconds=.03):
        if self.fd is not None and select.select([self.fd], [], [], seconds)[0]:
            try:
                chunk = os.read(self.fd, 65536)
            except OSError:
                chunk = b''
            self.output.extend(chunk)
            self.raw.write(chunk)
            self.raw.flush()
            assert len(self.output) <= 32*1024*1024, 'PTY output budget exceeded'

    def screen(self):
        return screen_text(bytes(self.output))

    def saved(self):
        return json.loads(self.checkpoint.read_text())

    def active(self):
        value = self.saved()
        name = value['projects'][value['active']]
        return next(row for row in value['project_state'] if row['name'] == name)

    def draft(self):
        return self.active()['draft']['input']

    def pane(self):
        # Only session pane row syntax, never historical transcript bytes.
        result = {}
        for match in re.finditer(rb'(>\s*)?([A-Z]126)\s+turns=', self.screen()):
            result[match.group(2).decode()] = bool(match.group(1))
        return result

    def wait(self, predicate, timeout=12):
        deadline = time.monotonic()+timeout
        while time.monotonic() < deadline:
            self.pump()
            try:
                value = predicate()
                if value:
                    return value
            except (FileNotFoundError, json.JSONDecodeError):
                pass
            if self.pid is not None:
                waited, status = os.waitpid(self.pid, os.WNOHANG)
                if waited:
                    self.pid, self.status = None, status
                    raise AssertionError('TUI exited before requested observation: '+str(status))
        raise AssertionError('observation deadline; see PTY and checkpoint evidence')

    def write(self, payload):
        self.event('input', base64=base64.b64encode(payload).decode())
        while payload:
            size = os.write(self.fd, payload)
            payload = payload[size:]

    def command(self, text):
        assert text.startswith(('/session ', '/project ', '/pane ', '/quit'))
        self.wait(lambda: not self.draft())
        self.write(text.encode())
        self.wait(lambda: self.draft() == text)
        # Model a deliberate Enter, outside the product's ordinary-paste window.
        deadline = time.monotonic()+.15
        while time.monotonic() < deadline:
            self.pump(.01)
        self.write(b'\r')
        self.event('command_submitted', text=text)

    def clear(self):
        count = 2*self.draft().count('\n')+1
        self.write(b'\x1b[1;5H'+b'\x0b'*count)
        self.wait(lambda: not self.draft())

    def snapshot(self, label):
        self.serial += 1
        prefix = '%02d-%s' % (self.serial, label)
        (self.evidence/(prefix+'.screen.txt')).write_bytes(self.screen())
        save(self.evidence/(prefix+'.checkpoint.json'), self.saved())
        self.event('snapshot', label=label, pane=self.pane())

    def observe(self, predicate, seconds=1.5):
        deadline = time.monotonic()+seconds
        while time.monotonic() < deadline:
            self.pump(.05)
            assert predicate(), 'state changed during bounded observation'
        self.event('observation_end', seconds=seconds, pane=self.pane())

    def close(self):
        self.clear()
        self.command('/quit')
        app_exit = self.wait(lambda: json.loads(self.app_exit.read_text()), timeout=12)
        while select.select([self.fd], [], [], .01)[0]:
            self.pump(0)
        attrs_after = termios.tcgetattr(self.slave)
        result = dict(app_exit=app_exit,
                      clean_exit=app_exit['returncode'] == 0,
                      entered_alternate=b'\x1b[?1049h' in self.output,
                      left_alternate=b'\x1b[?1049l' in self.output,
                      termios_restored=self.attrs_before == attrs_after,
                      termios_before=repr(self.attrs_before), termios_after=repr(attrs_after))
        save(self.evidence/'terminal-restoration.json', result)
        os.write(self.lifetime, b'x')
        os.close(self.lifetime)
        self.lifetime = None
        deadline = time.monotonic()+12
        while self.pid is not None and time.monotonic() < deadline:
            self.pump()
            waited, status = os.waitpid(self.pid, os.WNOHANG)
            if waited:
                self.pid, self.status = None, status
        assert self.pid is None, 'TUI did not exit before deadline'
        self.pump(.01)
        result['supervisor_exit_status'] = self.status
        save(self.evidence/'terminal-restoration.json', result)
        assert os.WIFEXITED(self.status) and os.WEXITSTATUS(self.status) == 0
        assert all(result[k] for k in ('clean_exit', 'entered_alternate', 'left_alternate', 'termios_restored'))

    def cleanup(self):
        # Only the directly spawned child PID is signalled/reaped. No process
        # names, ports, process-group guesses, global pkill or fixture deletion.
        if self.lifetime is not None:
            os.close(self.lifetime)
            self.lifetime = None
        if self.pid is not None:
            self.event('cleanup_sigterm', pid=self.pid)
            try:
                os.kill(self.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            deadline = time.monotonic()+4
            while self.pid is not None and time.monotonic() < deadline:
                self.pump()
                waited, status = os.waitpid(self.pid, os.WNOHANG)
                if waited:
                    self.pid, self.status = None, status
            if self.pid is not None:
                self.event('cleanup_sigkill', pid=self.pid)
                os.kill(self.pid, signal.SIGKILL)
                deadline = time.monotonic()+3
                while time.monotonic() < deadline:
                    self.pump()
                    waited, status = os.waitpid(self.pid, os.WNOHANG)
                    if waited:
                        self.pid, self.status = None, status
                        break
                assert self.pid is None, 'fixture child not reaped before deadline'
        for fd in (self.fd, self.slave):
            if fd is not None:
                os.close(fd)
        self.fd = self.slave = None
        self.raw.close()
        self.events.close()


def search_receipt(t, query):
    before = len(t.active()['messages'])
    t.command('/session search '+query)
    t.wait(lambda: any(m.get('text', '').startswith('session search:')
                       for m in t.active()['messages'][before:]))


def initial_search(t):
    t.clear()
    search_receipt(t, 'needle126')
    first = 'A126' if (t.paths['local']/'a.jsonl').exists() else 'B126'
    t.wait(lambda: {'B126', 'C126'} <= set(t.pane()) and 'O126' not in t.pane()
           and t.pane().get(first))
    t.wait(lambda: not t.draft())


def run_steps(t, paths, steps, results):
    t.start()
    initial_id = t.active()['name']
    if 'list' in steps:
        initial_search(t)
        t.command('/session list')
        t.wait(lambda: {'O126', 'B126', 'C126'} <= set(t.pane()))
        t.wait(lambda: any(m.get('text', '').startswith('sessions:') and 'G126' in m['text']
                           for m in t.active()['messages']))
        assert 'G126' not in t.pane(), 'global session incorrectly entered owner pane'
        t.snapshot('list-two-scopes')
        results['list'] = 'owner pane and global text receipt remain distinct; filter cleared'
    if 'selection' in steps:
        initial_search(t)
        t.write(b'\x1b[B')
        t.wait(lambda: t.pane().get('C126'))
        record_file(paths['local']/'a.jsonl', 'A126', paths['cwd'], 'needle126')
        t.wait(lambda: 'A126' in t.pane())
        assert t.pane().get('C126'), 'periodic refresh reset selected path to first row'
        t.observe(lambda: t.pane().get('C126'), 1.2)
        t.snapshot('selection-preserved-after-insert')
        t.write(b'\r')
        t.wait(lambda: t.active()['metadata']['session_path'] == str(paths['local']/'c.jsonl'))
        t.snapshot('keyboard-enter-selected-path')
        t.command('/session open '+str(paths['owner']))
        t.wait(lambda: t.active()['metadata']['session_path'] == str(paths['owner']))
        results['selection'] = 'Down selects C; insertion before it preserves C; Enter opens C path'
    if 'search-failure' in steps:
        initial_search(t)
        expected = t.pane()
        before = len(t.active()['messages'])
        bad = paths['local']/'bad.jsonl'
        bad.mkdir()  # Ordinary directory: the journal inspector must reject it.
        try:
            t.command('/session search failure126')
            t.wait(lambda: any('session search failed:' in m.get('text', '')
                               for m in t.active()['messages'][before:]))
            assert t.pane() == expected, 'failed search changed rows or selection'
            t.snapshot('search-failure-keeps-old-projection')
        finally:
            bad.rmdir()
        t.clear()
        results['search-failure'] = 'real invalid journal-directory entry; error visible, old rows/selection kept'
    if 'project' in steps:
        t.clear()
        t.command('/project open '+str(paths['other']))
        t.wait(lambda: t.active()['metadata']['cwd'] == str(paths['other']))
        other_id = t.active()['name']
        t.command('/session open '+str(paths['other_owner']))
        t.wait(lambda: t.active()['metadata']['session_path'] == str(paths['other_owner']))
        search_receipt(t, 'otherneedle126')
        t.wait(lambda: set(t.pane()) == {'D126'})
        before_other = len(t.active()['messages'])
        t.command('/project select '+initial_id)
        t.wait(lambda: t.active()['name'] == initial_id)
        t.command('/session search race126')
        query_at = time.monotonic()
        t.command('/project select '+other_id)
        switch_at = time.monotonic()
        t.wait(lambda: t.active()['name'] == other_id and set(t.pane()) == {'D126'})
        def no_leak():
            messages = t.active()['messages'][before_other:]
            return (t.active()['name'] == other_id and set(t.pane()) == {'D126'}
                    and not any('race126' in m.get('text', '') for m in messages))
        t.observe(no_leak, 2)
        t.snapshot('other-project-keeps-own-query')
        results['project'] = dict(status='entry-regression-observed', scan_overlap='UNPROVEN',
                                 query_submitted_at=query_at, switch_submitted_at=switch_at,
                                 claim='D owner/query projection intact; no late query receipt observed',
                                 limitation='No public scan-start barrier; cannot claim forced stale-result race or parallel responsiveness')
    t.close()
    results['exit'] = 'normal /quit restored alternate screen and original slave termios'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=('prepare', 'check', 'run'), default='prepare')
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--expect-binary-sha256')
    parser.add_argument('--out', type=Path, required=True, help='new independent evidence directory; must not exist')
    parser.add_argument('--steps', default=','.join(STEPS), help='comma-separated bounded steps; exit always checked')
    parser.add_argument('--pressure-mib', type=int, default=8, help='0..48 ordinary files, each under one MiB')
    args = parser.parse_args()
    steps = args.steps.split(',')
    if not steps or len(set(steps)) != len(steps) or any(s not in STEPS for s in steps):
        parser.error('steps must be unique names from '+','.join(STEPS))
    if not 0 <= args.pressure_mib <= 48:
        parser.error('pressure-mib must be in 0..48')
    binary = args.binary.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error('binary must be an existing executable file')
    actual = digest(binary)
    if args.expect_binary_sha256 and args.expect_binary_sha256 != actual:
        parser.error('binary hash mismatch; do not silently test another build')
    out = args.out.absolute()
    out.mkdir(parents=False, exist_ok=False)
    root = out/'fixture'
    result = dict(mode=args.mode, binary=str(binary), binary_sha256=actual, steps=steps,
                  real_pty_executed=False, checks={}, status='preparing',
                  gate_126_accepted=False, gate_085_accepted=False)
    terminal = None
    try:
        paths = prepare(root, args.pressure_mib)
        env = fixture_env(paths)
        save(out/'fixture-before.json', inventory(root, validate=True))
        save(out/'environment.json', dict(HOME_preserved=True, CODEX_HOME_preserved=True,
                                        ZENPI_HOME=str(paths['home'])))
        if args.mode in ('check', 'run'):
            result['preflight'] = preflight(binary, paths, env, out)
        if args.mode == 'run':
            assert digest(binary) == actual, 'binary changed after preflight'
            terminal = Terminal(binary, paths, env, out)
            run_steps(terminal, paths, steps, result['checks'])
        result['status'] = 'passed' if args.mode == 'run' else 'prepared-not-pty-tested'
    except BaseException as error:
        result.update(status='failed', error=repr(error), traceback=traceback.format_exc())
        raise
    finally:
        if terminal is not None:
            result['real_pty_executed'] = terminal.started
            try:
                if terminal.checkpoint.exists():
                    (out/'last-checkpoint.json').write_bytes(terminal.checkpoint.read_bytes())
                (out/'last-screen.txt').write_bytes(terminal.screen())
            finally:
                try:
                    terminal.cleanup()
                except BaseException as error:
                    result['cleanup_error'] = repr(error)
                    result['status'] = 'failed'
        try:
            save(out/'fixture-after.json', inventory(root))
            # All journals and checkpoints remain IN PLACE on success/failure.
            violations = []
            for p in root.rglob('*.jsonl'):
                if p.is_file():
                    for i, line in enumerate(p.read_text().splitlines()):
                        value = json.loads(line)
                        if value.get('event', {}).get('operation_kind') == 'provider':
                            violations.append(dict(path=str(p), line=i+1))
            result['provider_operations'] = violations
            if violations:
                result['status'] = 'failed'
        except BaseException as error:
            result['final_inventory_error'] = repr(error)
            result['status'] = 'failed'
        save(out/'result.json', result)
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 1 if result['status'] == 'failed' else 0


if __name__ == '__main__':
    raise SystemExit(main())

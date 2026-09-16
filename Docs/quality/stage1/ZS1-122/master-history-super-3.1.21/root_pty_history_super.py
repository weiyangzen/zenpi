"""One new local PTY run of the production TUI; zero model HTTP expected.

CSI-u is decoded by the captured crossterm parse.rs: codepoint;9u = Super,
codepoint;10u = Shift+Super. No old runner or product test helper is imported.
"""
import codecs
import fcntl
import hashlib
import http.server
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import threading
import time
import unicodedata

root = Path(__file__).resolve().parent
out = root/'root-pty-evidence'
out.mkdir(exist_ok=False)
workspace, config = out/'workspace', out/'config'
workspace.mkdir()
config.mkdir()
checks, sent, requests = [], [], []


class RejectHTTP(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        requests.append(dict(method='POST', path=self.path))
        self.send_error(503)

    def do_GET(self):
        requests.append(dict(method='GET', path=self.path))
        self.send_error(503)


class Screen:
    def __init__(self):
        self.rows = [[' ']*180 for _ in range(40)]
        self.x = self.y = 0
        self.pending = ''

    def feed(self, data):
        self.pending += data
        while self.pending:
            text = self.pending
            if text.startswith('\x1b'):
                if len(text) < 2:
                    return
                if text[1] == '[':
                    match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', text)
                    if not match:
                        return
                    self.pending = text[match.end():]
                    values = [int(v or 0) for v in match[1].lstrip('?').split(';')]
                    n, command = values[0] or 1, match[3]
                    if command in 'Hf':
                        self.y = min(39, max(0, n-1))
                        self.x = min(179, max(0, (values[1] or 1)-1 if len(values)>1 else 0))
                    elif command == 'A': self.y = max(0, self.y-n)
                    elif command == 'B': self.y = min(39, self.y+n)
                    elif command == 'C': self.x = min(179, self.x+n)
                    elif command == 'D': self.x = max(0, self.x-n)
                    elif command == 'G': self.x = min(179, n-1)
                    elif command == 'd': self.y = min(39, n-1)
                    elif command == 'J':
                        if values[0] in (2, 3): self.rows = [[' ']*180 for _ in range(40)]
                        elif values[0] == 0:
                            self.rows[self.y][self.x:] = [' ']*(180-self.x)
                            for y in range(self.y+1, 40): self.rows[y] = [' ']*180
                    elif command == 'K':
                        a, b = (0, 180) if values[0] == 2 else ((0, self.x+1) if values[0] == 1 else (self.x, 180))
                        self.rows[self.y][a:b] = [' ']*(b-a)
                    continue
                if text[1] == ']':
                    match = re.search(r'\x07|\x1b\\', text[2:])
                    if not match: return
                    self.pending = text[2+match.end():]
                    continue
                if text[1] in '()':
                    if len(text)<3: return
                    self.pending=text[3:]
                    continue
                self.pending = text[2:]
                continue
            self.pending = text[1:]
            ch = text[0]
            if ch == '\r': self.x = 0
            elif ch == '\n': self.y = min(39, self.y+1)
            elif ch == '\b': self.x = max(0, self.x-1)
            elif ord(ch) >= 32 and not unicodedata.combining(ch):
                self.rows[self.y][self.x] = ch
                width = 2 if unicodedata.east_asian_width(ch) in 'WF' else 1
                if width == 2 and self.x < 179: self.rows[self.y][self.x+1] = ''
                self.x = min(179, self.x+width)

    def text(self):
        return '\n'.join(''.join(row) for row in self.rows)

    def query(self):
        line = next((x for x in self.text().splitlines() if 'History search:' in x), '')
        match = re.search(r'History search: (.*?) · (?:match|no match)', line)
        return match[1] if match else None

    def prompt(self):
        lines = self.text().splitlines()
        at = next((i for i, line in enumerate(lines) if 'History search:' in line or ' Prompt ' in line), None)
        return '\n'.join(lines[at+1:-1]) if at is not None else ''


binary = root/'root-bin/zenpi'
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), RejectHTTP)
server_thread = threading.Thread(target=server.serve_forever, daemon=True)
server_thread.start()
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 180, 0, 0))
original_terminal = termios.tcgetattr(slave)
env = {k: os.environ[k] for k in ('HOME', 'PATH', 'LANG', 'TMPDIR') if k in os.environ}
env.update(TERM='xterm-256color', ZENPI_HOME=str(config), ZENPI_BACKEND='openai', ZENPI_BASE_URL=f'http://127.0.0.1:{server.server_port}/v1', ZENPI_API_KEY='local-fixture-only', ZENPI_MODEL='gpt-4.1')
argv = [str(binary), '--tui', '--session', str(workspace/'session.jsonl')]


def child_session():
    os.setsid()
    fcntl.ioctl(0, termios.TIOCSCTTY, 0)


report_read, report_write = os.pipe()
ack_read, ack_write = os.pipe()
keeper_argv = [sys.executable, str(root/'pty_keeper.py'), str(report_write), str(ack_read), *argv]
process = subprocess.Popen(keeper_argv, cwd=workspace, env=env, stdin=slave, stdout=slave, stderr=slave, preexec_fn=child_session, close_fds=True, pass_fds=(report_write, ack_read))
os.close(report_write)
os.close(ack_read)
child_result = {}


def child_exited():
    if child_result:
        return True
    if select.select([report_read], [], [], 0)[0]:
        data = os.read(report_read, 4096)
        if data:
            child_result.update(json.loads(data))
    return bool(child_result)

screen = Screen()
decoder = codecs.getincrementaldecoder('utf-8')('replace')
raw = bytearray()
started = time.monotonic()


def pump(duration):
    end = time.monotonic()+duration
    while time.monotonic()<end:
        if select.select([master], [], [], min(.05, max(0, end-time.monotonic())))[0]:
            try: data = os.read(master, 65536)
            except OSError: return
            if not data: return
            raw.extend(data)
            screen.feed(decoder.decode(data))
            (out/'terminal.raw').write_bytes(raw)


def until(predicate, timeout=8):
    end = time.monotonic()+timeout
    while time.monotonic()<end:
        pump(.08)
        if predicate(): return True
        if process.poll() is not None: return False
    return False


def send(label, data):
    sent.append(dict(label=label, elapsed=time.monotonic()-started, hex=data.hex(), bytes=len(data)))
    (out/'input-bytes.json').write_text(json.dumps(sent, indent=2)+'\n')
    os.write(master, data)
    pump(.22)


def paste(label, text):
    send(label, b'\x1b[200~'+text.encode()+b'\x1b[201~')


def check(label, ok):
    checks.append(dict(name=label, pass_=bool(ok)))
    (out/'checks-progress.json').write_text(json.dumps(checks, indent=2)+'\n')
    (out/f'{len(checks):02d}-screen.txt').write_text(screen.text())
    if not ok: raise AssertionError(label)


def draft():
    value = json.loads((config/'project-tabs.json').read_text())
    active = value['projects'][value['active']]
    return next(row['draft'] for row in value['project_state'] if row['name'] == active)


error = None
try:
    check('production TUI ready', until(lambda: 'Prompt' in screen.text()))
    for command in ['/help', '/help input', '/status']:
        paste('seed '+command, command)
        send('submit control only', b'\r')
    check('control history caused no HTTP', not requests)
    payload = '保留'*501
    paste('folded unsent draft', payload)
    send('fold cursor to start', b'\x1b[D')
    pump(.4)
    before = draft()
    (out/'before-draft.json').write_text(json.dumps(before, ensure_ascii=False, indent=2)+'\n')
    check('real checkpoint contains exact folded draft and cursor', before['input'] == payload and before['cursor'] == 0 and len(before['paste_folds']) == 1)
    send('Ctrl-R search', b'\x12')
    send('Unicode query', '界'.encode())
    check('Unicode reaches history query', screen.query() == '界')
    send('Backspace Unicode', b'\x7f')
    send('Shift-A CSI-u', b'\x1b[65;2u')
    check('Shift character reaches history query', screen.query() == 'A')
    send('Backspace shifted character', b'\x7f')
    send('normal history query', b'help')
    check('normal query selects latest matching command', screen.query() == 'help' and '/help input' in screen.prompt())
    send('Super-x supported CSI-u', b'\x1b[120;9u')
    check('Super character ignored in real event reader', screen.query() == 'help' and '/help input' in screen.prompt())
    send('Super-Shift-X supported CSI-u', b'\x1b[88;10u')
    check('Super+Shift character ignored in real event reader', screen.query() == 'help' and '/help input' in screen.prompt())
    send('plain sentinel after modified sequences', b'p')
    check('reader continues after both modified sequences', screen.query() == 'helpp')
    send('remove sentinel', b'\x7f')
    send('Ctrl-R older match', b'\x12')
    check('Ctrl-R selects older command', screen.query() == 'help' and '/help' in screen.prompt() and '/help input' not in screen.prompt())
    send('Esc restores original draft', b'\x1b')
    pump(.4)
    restored = draft()
    (out/'after-escape-draft.json').write_text(json.dumps(restored, ensure_ascii=False, indent=2)+'\n')
    fields = ['input', 'cursor', 'paste_folds', 'next_paste_id']
    check('Esc preserves exact draft fold and cursor durably', all(restored[k] == before[k] for k in fields) and screen.query() is None)
    send('Ctrl-R accept exercise', b'\x12')
    send('query for acceptance', b'help')
    send('Enter accepts without submitting', b'\r')
    pump(.4)
    accepted = draft()
    (out/'accepted-draft.json').write_text(json.dumps(accepted, indent=2)+'\n')
    check('Enter only accepts selected command into draft', screen.query() is None and accepted['input'] == '/help input' and accepted['paste_folds'] == [])
    send('clear accepted draft', b'\x15')
    paste('quit control', '/quit')
    send('explicit quit submit', b'\r')
    check('normal exit and reap', until(child_exited) and child_result['exit_code'] == 0 and child_result['reaped'])
except Exception as exc:
    error = repr(exc)
finally:
    # The keeper owns the controlling PTY until flags have been observed.
    # A exited PTY session leader would make macOS tcgetattr return ENOTTY.
    if not child_exited() and process.poll() is None:
        process.terminate()
        until(child_exited, 4)
    pump(.1)
    check_mask = termios.ICANON | termios.ECHO | termios.ISIG
    try:
        restored_terminal = termios.tcgetattr(slave)
        flags_ok = (original_terminal[3] & check_mask) == (restored_terminal[3] & check_mask)
        terminal_observation = dict(original=original_terminal[3], restored=restored_terminal[3], mask=check_mask)
    except Exception as exc:
        flags_ok = False
        terminal_observation = dict(error=repr(exc))
    checks.append(dict(name='terminal flags restored before keeper exit', pass_=flags_ok))
    checks.append(dict(name='alternate screen left', pass_=b'\x1b[?1049l' in raw))
    try:
        os.write(ack_write, b'x')
    except OSError:
        pass
    os.close(ack_write)
    try:
        process.wait(timeout=4)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=4)
    checks.append(dict(name='keeper exited and reaped', pass_=process.returncode == 0))
    pump(.1)
    os.close(report_read)
    os.close(master)
    os.close(slave)
    server.shutdown()
    server.server_close()
    server_thread.join(timeout=2)
    checks.append(dict(name='zero final HTTP requests', pass_=not requests))
    checks.append(dict(name='local HTTP server stopped', pass_=not server_thread.is_alive()))
    (out/'terminal.raw').write_bytes(raw)
    (out/'final-screen.txt').write_text(screen.text())
    result = dict(checks=checks, error=error, child=child_result, keeper=dict(pid=process.pid, exit_code=process.returncode, argv=keeper_argv), terminal_observation=terminal_observation, argv=argv, cwd=str(workspace), env_names=sorted(env), input_encoding='captured vendored crossterm CSI-u parser; real terminal bytes', http_requests=requests, actual_pty=True, binary=dict(bytes=binary.stat().st_size, sha256=hashlib.sha256(binary.read_bytes()).hexdigest()), duration_seconds=time.monotonic()-started)
    (out/'result.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result, indent=2))
    sys.exit(0 if error is None and all(c['pass_'] for c in checks) else 1)

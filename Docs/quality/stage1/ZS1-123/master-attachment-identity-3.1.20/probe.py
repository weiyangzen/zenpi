"""Real JSONL and PTY attachment identity; only a loopback Responses fixture."""
from pathlib import Path
import argparse, base64, hashlib, json, os, sys, threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, str(Path.cwd() / 'tools'))
from tui_session_new_smoke import Jsonl
from tui_project_workspace_smoke import Terminal

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--evidence', type=Path, required=True)
args = parser.parse_args()
binary, evidence = args.binary.resolve(), args.evidence.resolve()
fixture = evidence.with_suffix('.fixture')
fixture.mkdir()
requests, errors, wires, terminals = [], [], [], []
cases = [(r'note\report.txt', b'LITERAL_FILE_EXPECTED\n'),
         ('note/report.txt', b'DIRECTORY_FILE_EXPECTED_WITH_DIFFERENT_BYTES\n')]
checks, receipts, roots = {}, {}, []
result = dict(binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), checks=checks)

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        try:
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            reply = f'ATTACHMENT_REPLY_{len(requests)}'
            events = [{'type': 'response.output_text.delta', 'delta': reply},
                      {'type': 'response.completed', 'response': {'id': reply, 'status': 'completed'}}]
            raw = ''.join('data: ' + json.dumps(e) + '\n\n' for e in events).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
        except BaseException as error:
            errors.append(repr(error))

server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()

def setup(name):
    root = fixture / name
    for path in (root / 'initial/note', root / 'sessions', root / 'config'):
        path.mkdir(parents=True)
    for path, content in cases:
        (root / 'initial' / path).write_bytes(content)
    (root / 'initial/linked.txt').symlink_to(root / 'initial' / cases[0][0])
    assert (root / 'initial' / cases[0][0]).stat().st_ino != (root / 'initial' / cases[1][0]).stat().st_ino
    (root / 'config/config.toml').write_text(
        f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\n'
        'wire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('ZENPI_', 'OPENAI_'))
           and k.lower() not in ('http_proxy', 'https_proxy', 'all_proxy', 'no_proxy')}
    env.update(ZENPI_HOME=str(root / 'config'), TERM='xterm-256color')
    roots.append(root)
    return root, env

def inspect_request(label, body, path, expected):
    parts = [part for row in body.get('input', []) if row.get('role') == 'user'
             and isinstance(row.get('content'), list) for part in row['content']
             if part.get('type') == 'input_file']
    checks[label + '_one_materialized_file'] = len(parts) == 1
    checks[label + '_exact_bytes'] = len(parts) == 1 and base64.b64decode(parts[0]['file_data'].split(',', 1)[1]) == expected
    checks[label + '_filename'] = len(parts) == 1 and parts[0].get('filename') == Path(path).name

def inspect_journal(label, root):
    raw = (root / 'sessions/initial.jsonl').read_text()
    rows = [json.loads(line) for line in raw.splitlines()]
    found = []
    def walk(value):
        if isinstance(value, dict):
            if isinstance(value.get('attachments'), list):
                found.extend(value['attachments'])
            for child in value.values():
                walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)
    walk(rows)
    checks[label + '_durable_identity'] = all(any(
        a.get('path') == path and a.get('sha256') == hashlib.sha256(content).hexdigest()
        and a.get('size_bytes') == len(content) for a in found) for path, content in cases)
    checks[label + '_journal_excludes_file_body'] = all(content.decode().strip() not in raw for _, content in cases)

try:
    root, env = setup('jsonl')
    wire = Jsonl(binary, root, env)
    wires.append(wire)
    for index, (path, expected) in enumerate(cases):
        value = wire.command(f'attach-{index}', '/attach ' + path.replace(chr(92), chr(92) * 2))
        receipts[str(index)] = value
        assert value['success'], value
        data = value['data']
        checks[f'jsonl_{index}_receipt_identity'] = (data['path'] == path and data['size_bytes'] == len(expected)
            and data['sha256'] == hashlib.sha256(expected).hexdigest())
        if index == 0:
            before = len(requests)
            for j, denied in enumerate(('linked.txt', '../escape.txt', 'missing.txt')):
                response = wire.command(f'deny-{j}', '/attach ' + denied)
                checks[f'jsonl_denied_{j}'] = response['success'] is False
            checks['jsonl_attachment_commands_zero_http'] = len(requests) == before == 0
        wire.send(f'prompt-{index}', type='prompt', text=f'JSONL_ATTACH_{index}')
        response = wire.response(f'prompt-{index}')
        assert response['success'], response
        inspect_request(f'jsonl_{index}', requests[-1], path, expected)
    wire.close()
    inspect_journal('jsonl', root)
    root, env = setup('tui')
    terminal = Terminal(binary, root, env)
    terminals.append(terminal)
    for index, (path, expected) in enumerate(cases):
        before = len(requests)
        terminal.command('/attach ' + path.replace(chr(92), chr(92) * 2))
        terminal.wait(lambda: terminal.draft() == '')
        checks[f'tui_{index}_stage_zero_http'] = len(requests) == before
        terminal.command(f'TUI_ATTACH_{index}')
        terminal.expect(f'ATTACHMENT_REPLY_{before + 1}'.encode())
        terminal.wait(lambda: terminal.draft() == '')
        inspect_request(f'tui_{index}', requests[-1], path, expected)
    terminal.close()
    inspect_journal('tui', root)
    checks['exactly_four_provider_calls'] = len(requests) == 4
    checks['terminal_restored'] = b'\x1b[?1049l' in terminals[0].output
    assert not errors, errors
    assert all(checks.values()), [key for key, passed in checks.items() if not passed]
    result['status'] = 'passed'
except BaseException as error:
    result.update(status='failed', error=repr(error))
    raise
finally:
    for wire in wires:
        if wire.process.poll() is None:
            wire.kill()
    for terminal in terminals:
        terminal.cleanup()
    server.shutdown()
    server.server_close()
    result.update(http_count=len(requests), jsonl_processes=len(wires), tui_processes=len(terminals), server_errors=errors, receipts=receipts)
    evidence.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    evidence.with_suffix('.http.json').write_text(json.dumps(requests, ensure_ascii=False, indent=2) + '\n')
    evidence.with_suffix('.jsonl.json').write_text(json.dumps([{'returncode': w.process.returncode, 'rows': w.rows, 'stderr': w.errors} for w in wires], ensure_ascii=False, indent=2) + '\n')
    evidence.with_suffix('.pty.log').write_bytes(b'\nPROCESS\n'.join(bytes(t.output) for t in terminals))
    for root in roots:
        path = root / 'sessions/initial.jsonl'
        if path.exists():
            evidence.with_suffix('.' + root.name + '.journal.jsonl').write_bytes(path.read_bytes())

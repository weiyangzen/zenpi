"""Pure offline checks. No PTY, HTTP, Cargo or TUI process is started."""
from pathlib import Path
import ast
import hashlib
import json
import os
import subprocess
import sys
import traceback
import probe

P = Path(__file__).resolve().parent
out = P/'offline-selfcheck'
out.mkdir(exist_ok=False)
result = {'real_pty_executed': False, 'HTTP_executed': False, 'Cargo_executed': False,
          'script_sha256': hashlib.sha256((P/'probe.py').read_bytes()).hexdigest(), 'checks': {}}

def forbidden(*args, **kwargs):
    raise AssertionError('offline check attempted to create a PTY or fork a TUI')

probe.pty.openpty = forbidden
probe.os.fork = forbidden
try:
    for name in ('probe.py', 'screen_adapter.py', 'selfcheck.py'):
        compile((P/name).read_bytes(), name, 'exec')
    result['checks']['syntax'] = True
    paths = probe.prepare(out/'fixture', 2)
    fixture = probe.inventory(out/'fixture', validate=True)
    probe.save(out/'fixtures.json', fixture)
    assert fixture['total_session_bytes'] < 3*1024*1024
    assert 48*(4*(240*1024+1024)+1024)+1024*1024 < probe.LIMIT
    result['checks']['fixture_budget_and_schema'] = True
    env = probe.fixture_env(paths)
    assert env.get('HOME') == os.environ.get('HOME')
    assert env.get('CODEX_HOME') == os.environ.get('CODEX_HOME')
    result['checks']['HOME_CODEX_HOME_preserved'] = True
    raw = b'\x1b[2J\x1b[2;2H> C126  turns=1\x1b[3;2H  B126  turns=1\x1b[5;2Hsessions: {"session_id":"G126"}'
    class Fake:
        def screen(self):
            return probe.screen_text(raw)
    assert probe.Terminal.pane(Fake()) == {'C126': True, 'B126': False}
    assert b'C126' not in probe.screen_text(raw+b'\x1b[2J')
    result['checks']['cursor_addressed_pane_parser_not_transcript_substring'] = True
    rows = json.loads((P/'adapter-read.json').read_text())
    for row in rows:
        d = (P/row['snapshot']).read_bytes()
        assert hashlib.sha256(d).hexdigest() == row['sha256']
        assert len(d.splitlines()) == row['lines']
    source = (P/'read-tui_project_workspace_smoke.py').read_text()
    node = next(n for n in ast.parse(source).body if isinstance(n, ast.FunctionDef) and n.name == 'screen_text')
    exact = '\n'.join(source.splitlines()[node.lineno-1:node.end_lineno])+'\n'
    assert (P/'screen_adapter.py').read_text().endswith(exact)
    result['checks']['fully_read_adapter_exact_screen_extraction'] = True
    binary = Path('/Users/wangweiyang/GitHub/zenpi/target/release/zenpi')
    profile = probe.sandbox_profile(binary, out/'fixture')
    (out/'sandbox.sb').write_text(profile+'\n')
    outside = out/'outside-fixture.txt'
    outside.write_text('deny this private selfcheck file; not user data\n')
    commands = [('sandbox-syntax', ['/usr/bin/true'], 0),
                ('sandbox-fixture-read', ['/bin/cat', str(paths['local']/'b.jsonl')], 0),
                ('sandbox-outside-denied', ['/bin/cat', str(outside)], 1)]
    for name, command, expected in commands:
        argv = ['/usr/bin/sandbox-exec', '-f', str(out/'sandbox.sb')]+command
        p = subprocess.run(argv, capture_output=True, timeout=10)
        (out/(name+'.stdout')).write_bytes(p.stdout)
        (out/(name+'.stderr')).write_bytes(p.stderr)
        probe.save(out/(name+'.run.json'), {'argv': argv, 'exit_code': p.returncode,
                                          'expected_exit_code': expected, 'no_PTY_HTTP_Cargo': True})
        assert p.returncode == expected, (name, p.returncode, p.stderr)
    result['checks']['sandbox_syntax_fixture_read_and_outside_denial'] = True
    result['status'] = 'passed-offline-only'
except BaseException as error:
    result.update(status='failed', error=repr(error), traceback=traceback.format_exc())
    raise
finally:
    probe.save(out/'result.json', result)
print(json.dumps(result, indent=2))

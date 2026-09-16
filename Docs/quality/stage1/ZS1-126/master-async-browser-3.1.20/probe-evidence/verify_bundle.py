"""Read-only verification; no imports of executable probe code, PTY or child processes."""
from pathlib import Path
import ast
import hashlib
import json
P=Path(__file__).resolve().parent
m=json.loads((P/'manifest.json').read_text())
for item in m['files']:
 p=P/item['path'];assert p.is_file() and not p.is_symlink()
 d=p.read_bytes();assert len(d)==item['bytes'] and hashlib.sha256(d).hexdigest()==item['sha256'],item['path']
for name in ('probe.py','screen_adapter.py','selfcheck.py','verify_bundle.py'):
 compile((P/name).read_bytes(),name,'exec')
for item in json.loads((P/'adapter-read.json').read_text()):
 d=(P/item['snapshot']).read_bytes();assert len(d.splitlines())==item['lines'] and hashlib.sha256(d).hexdigest()==item['sha256']
source=(P/'read-tui_project_workspace_smoke.py').read_text()
node=next(n for n in ast.parse(source).body if isinstance(n,ast.FunctionDef) and n.name=='screen_text')
assert (P/'screen_adapter.py').read_text().endswith('\n'.join(source.splitlines()[node.lineno-1:node.end_lineno])+'\n')
for path in P.glob('owner-*.fragments.json'):
 for item in json.loads(path.read_text())['fragments']:
  assert hashlib.sha256(item['text'].encode()).hexdigest()==item['sha256']
check=json.loads((P/'check-installed/result.json').read_text())
assert check['status']=='prepared-not-pty-tested' and check['real_pty_executed'] is False and not check['provider_operations']
assert json.loads((P/'check-installed/fixture-before.json').read_text())==json.loads((P/'check-installed/fixture-after.json').read_text())
selfcheck=json.loads((P/'offline-selfcheck/result.json').read_text())
assert selfcheck['status']=='passed-offline-only' and not selfcheck['real_pty_executed'] and not selfcheck['HTTP_executed'] and not selfcheck['Cargo_executed']
assert hashlib.sha256((P/'probe.py').read_bytes()).hexdigest()==selfcheck['script_sha256']
for path in (P/'offline-selfcheck').glob('*.run.json'):
 r=json.loads(path.read_text());assert r['exit_code']==r['expected_exit_code']
assert 'Operation not permitted' in (P/'offline-selfcheck/sandbox-outside-denied.stderr').read_text()
print(json.dumps({'status':'bundle-verified-offline-only','files':len(m['files']),'PTY_run':False,'126_accepted':False,'085_accepted':False}))

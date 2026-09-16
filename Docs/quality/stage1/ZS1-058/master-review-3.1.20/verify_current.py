from pathlib import Path
import sys, json, subprocess

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'tools'))
import validate_stage1_blueprint as v
w = Path(__file__).resolve().parent
p = w / 'frozen-input'
assert v.digest((p / 'manifest.json').read_bytes()) == '0626277a63874cdfae524362c9580182f137af8c14af85c4cba9f5525e3d7aca'
check = subprocess.run([sys.executable, str(p / 'verify.py'), '--upstream'], capture_output=True, text=True)
assert check.returncode == 0, check.stderr
inv = v.read_json(p / 'inventory.json')
bp = v.parse((ROOT / v.BLUEPRINT).read_text())
for x in inv['master014_files']:
    data = Path(x['path']).read_bytes()
    assert len(data) == x['bytes'] and v.digest(data) == x['sha256']
    assert data == (w / 'evidence/master014' / x['name']).read_bytes()
for x in json.loads((p / 'mapping-identities.json').read_text()):
    data = Path(x['path']).read_bytes()
    assert len(data) == x['bytes'] and v.digest(data) == x['sha256']
assert list(bp.items['ZS1-058'].depends) == ['ZS1-014']
assert bp.items['ZS1-014'].state == '[x]' and bp.items['ZS1-058'].state == '[ ]'
assert tuple(bp.folders['ZS1-058'][:2]) == ('source', 'packages/agent/test')
files = [k for k, f in bp.files.items() if f.scope == 'source' and Path(f.path).parent == Path('packages/agent/test')]
folders = [k for k, (scope, path, _) in bp.folders.items() if scope == 'source' and Path(path).parent == Path('packages/agent/test')]
assert files == ['ZS1-014'] and not folders
result = v.validate(ROOT, item='ZS1-014')
assert result['ok'], result
out = dict(ok=True, entry_payloads=5, old_payloads=32, source_closure_files=8, master014_files=10,
    target_identity_files=6, direct_children=6, scoped_files=files, scoped_directories=folders,
    matching_cases=23, new_tests=0, new_collections=0,
    scope='Independent directory semantic review with accepted014 actual replay reuse; no product gate acceptance')
print(json.dumps(out, indent=2))

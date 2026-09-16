"""Read-only verification of the portable independent-review package."""
import hashlib, json, pathlib, stat
root=pathlib.Path(__file__).resolve().parent
def digest(p):
    data=p.read_bytes(); return {'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
m=json.loads((root/'manifest.json').read_text())
assert m['product_changes']==[] and not m['whole_product_acceptance']
actual={str(p.relative_to(root)) for p in root.rglob('*') if stat.S_ISREG(p.lstat().st_mode) and p!=root/'manifest.json'}
assert actual==set(m['artifacts'])
for rel, expected in m['artifacts'].items():
    assert not pathlib.Path(rel).is_absolute() and '..' not in pathlib.Path(rel).parts
    assert digest(root/rel)==expected, rel
assert digest(root/'context/src/render.rs')['sha256']==m['reviewed_render_sha256']
assert (root/'context/src/render.rs').read_bytes()==(root/'input-package/after/src/render.rs').read_bytes()
assert (root/'context/.ops/work/harness/Cargo.lock').read_bytes()==(root/'input-package/executed-harness/Cargo.lock').read_bytes()
for name, snippets in [('independent-tests-01',['8 passed; 0 failed']),('candidate-existing-tests',['23 passed; 0 failed','4 passed; 0 failed']),('candidate-integrity',['"passed": true'])]:
    receipt=json.loads((root/'commands'/f'{name}.json').read_text())
    log=root/'commands'/f'{name}.log'
    assert receipt['exit_code']==0 and receipt['HOME_preserved'] and receipt['CODEX_HOME_preserved']
    assert digest(log)=={'bytes':receipt['bytes'],'sha256':receipt['sha256']}
    assert all(snippet in log.read_text() for snippet in snippets)
assert json.loads((root/'preservation-result.json').read_text())['passed']
print(json.dumps({'passed':True,'artifacts':len(actual),'manifest_sha256':digest(root/'manifest.json')['sha256'],'product_changes':[],'runtime_executions_by_verifier':0,'scope':'renderer review only'}))

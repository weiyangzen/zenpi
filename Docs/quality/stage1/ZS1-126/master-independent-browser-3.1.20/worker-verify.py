from pathlib import Path
import hashlib, json, os, stat

P = Path(__file__).resolve().parent

def info(p):
    assert stat.S_ISREG(p.lstat().st_mode), str(p)
    b = p.read_bytes()
    return {'bytes': len(b), 'sha256': hashlib.sha256(b).hexdigest()}

def read(name):
    return json.loads((P / name).read_text())

def inventory(root):
    result = {}
    for directory, dirs, files in os.walk(root, followlinks=False):
        for n in dirs:
            assert not Path(directory, n).is_symlink()
        for n in files:
            p = Path(directory, n)
            result[str(p.relative_to(root))] = info(p)
    return result

manifest = read('manifest.json')
expected = {}
for row in manifest['files']:
    rel = Path(row['path'])
    assert not rel.is_absolute() and '..' not in rel.parts
    assert row['path'] not in expected
    expected[row['path']] = {k:row[k] for k in ['bytes','sha256']}
actual = inventory(P)
actual.pop('manifest.json')
assert actual == expected
binding = read('build-binding.json')
assert info(P/'before-tui.rs') == binding['before'] == manifest['source']
assert (P/'context/src/tui.rs').read_bytes() == (P/'before-tui.rs').read_bytes() + binding['test_only_append'].encode()
assert info(P/'context/src/tui.rs') == binding['compiled_tui']
context = read('context-manifest.json')['files']
assert len(context) == 128
for rel, expected in context.items():
    assert info(P/'context'/rel) == (binding['compiled_tui'] if rel == 'src/tui.rs' else expected)
for row in read('read-bindings.json')['reads']:
    p = P/row['path']
    assert info(p) == row['source']
    lines = p.read_bytes().splitlines(keepends=True)
    assert 1 <= row['start'] <= row['end'] <= len(lines)
    chunk = b''.join(lines[row['start']-1:row['end']])
    assert len(chunk) == row['bytes']
    assert hashlib.sha256(chunk).hexdigest() == row['sha256']
assert inventory(P/'input-A') == read('A-inventory.json')
for row in read('input-A/manifest.json')['files']:
    assert info(P/'input-A'/row['path']) == {k:row[k] for k in ['bytes','sha256']}
for run in read('run-source-bindings.json')['runs']:
    assert info(P/run['test_source']) == run['test_source_identity']
    assert run['compiled_tui'] == binding['compiled_tui']
    log = P/'commands'/(run['name']+'.log')
    receipt = read('commands/'+run['name']+'.json')
    assert receipt['exit_code'] == 0
    assert info(log) == run['log'] == {k:receipt[k] for k in ['bytes','sha256']}
    assert info(P/'commands'/(run['name']+'.json')) == run['receipt']
    assert '--offline' in receipt['argv'] and '--locked' in receipt['argv']
    assert 'test result: ok. 8 passed; 0 failed; 0 ignored;' in log.read_text()
assert read('commands/independent-format-check.json')['exit_code'] == 0
preserved = read('preservation-after.json')
assert preserved['all_byte_identical'] and preserved['main_tui_byte_identical']
assert preserved['old_ready_packages'] == 78 and preserved['old_ready_regular_files'] == 11370
assert preserved['tracked_regular_files'] == 145 and preserved['product_patch'] is None
print(json.dumps({'status':'passed','manifest_files':len(manifest['files']),'compile_inputs':128,'new_cases':8,'new_product_patch':False,'scope':'Offline frozen artifact verification, not fresh Cargo execution or live external preservation recheck'}))

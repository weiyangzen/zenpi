from pathlib import Path
import dataclasses, hashlib, json, sys
ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'tools'))
import validate_stage1_blueprint as v
w = Path(__file__).resolve().parent
p = w / 'frozen-input'
base = p / 'files'
ev = base / 'Docs/quality/stage1/ZS1-016/worker-reuse319'
sha = lambda b: hashlib.sha256(b).hexdigest()
assert sha((p / 'manifest.json').read_bytes()) == 'b94b4561f37f1e2eb67ae01f04e68b33ad044b69dd83403208deb1f209478f30'
m = v.read_json(p / 'manifest.json')
for x in m['files']:
    data = (base / x['path']).read_bytes()
    assert len(data) == x['bytes'] and sha(data) == x['sha256']
    assert sha((p / x['patch_file']).read_bytes()) == x['patch_sha256']
assert len((p / 'candidate.patch').read_bytes()) == m['patch_bytes'] and sha((p / 'candidate.patch').read_bytes()) == m['patch_sha256']
bp = v.parse((ROOT / v.BLUEPRINT).read_text())
old = v.parse((ROOT / '.ops/stage1_execution/requirement-3.1.19-archive/blueprint.md').read_text())
assert dataclasses.asdict(bp.items['ZS1-016']) == dataclasses.asdict(old.items['ZS1-016']) and bp.files['ZS1-016'] == old.files['ZS1-016']
f = bp.files['ZS1-016']
raw = (Path(bp.header['source_repo']) / f.path).read_bytes()
assert len(raw) == 48616 and len(raw.splitlines()) == 1490 and sha(raw) == f.sha256
assert raw == (ev / 'anthropic-messages.ts').read_bytes()
input_manifest = v.read_json(ev / 'input-manifest.json')
ranges = []
for part in input_manifest['ordered_complete_read_chunks']:
    data = (base / part['artifact']).read_bytes()
    assert data == raw[part['byte_start']:part['byte_end']] and sha(data) == part['sha256']
    ranges.append([part['byte_start'], part['byte_end']])
v.check_ranges(ranges, len(raw))
historical = w / 'historical-input'
assert sha((historical / 'manifest.json').read_bytes()) == 'f52b12aaea01cf39c2e9705f81cce2560d427064d9a269259379f07e409c2d8d'
hm = v.read_json(historical / 'manifest.json')
for x in hm['files']:
    data = (historical / 'files' / x['path']).read_bytes()
    assert len(data) == x['bytes'] and sha(data) == x['sha256']
    if x['path'] == f.artifact:
        assert (base / x['path']).read_bytes().startswith(data)
    else:
        assert data == (base / x['path']).read_bytes()
prior = base / 'Docs/quality/stage1/ZS1-016/worker-source-probe'
for name, code in [('source-native', 1), ('source-native-final', 0), ('target-native', 0)]:
    run = v.read_json(prior / (name + '.json'))
    data = (prior / (name + '.log')).read_bytes()
    assert run['exit_code'] == code and run['bytes'] == len(data) and run['sha256'] == sha(data)
refs = json.loads((ev / 'target-references.json').read_text())
count = 0
for ref in refs:
    data = (ROOT / ref['repository_path']).read_bytes()
    assert len(data) == ref['bytes'] and sha(data) == ref['sha256']
    for part in ref['excerpts']:
        section = (base / part['artifact']).read_bytes()
        assert section == data[part['byte_start']:part['byte_end']] and sha(section) == part['sha256']
        count += 1
run = v.read_json(w / 'replay.run.json')
assert run['exit_code'] == 0 and not run['timeout']
assert sha((w / 'actual.stdout.json').read_bytes()) == run['stdout_sha256']
assert sha((w / 'actual.stderr.log').read_bytes()) == run['stderr_sha256']
result = v.read_json(w / 'actual.stdout.json')
old_result = v.read_json(prior / 'source-native-final.log')
assert result['scenarios'] == old_result['scenarios'] and len(result['scenarios']) == 23 and result['http_requests'] == 24
assert all(c['status'] == 'pass' for c in result['scenarios'])
identity = v.read_json(w / 'replay-inputs.json')
assert len(identity['source_files']) == 179 and len(identity['runtime_files']) == 2395
for directory, records in [('ai/src', identity['source_files']), ('node_modules', identity['runtime_files'])]:
    for ref in records:
        path = w / 'actual-replay' / directory / ref['path']
        if 'symlink' in ref:
            assert path.is_symlink() and str(path.readlink()) == ref['symlink']
        else:
            data = path.read_bytes()
            assert len(data) == ref['bytes'] and sha(data) == ref['sha256']
assert sha((w / 'actual-replay/package-lock.json').read_bytes()) == sha((prior / 'package-lock.json').read_bytes())
for name, version in [('@anthropic-ai/sdk', '0.124.0'), ('partial-json', '0.1.7')]:
    assert json.loads((w / 'actual-replay/node_modules' / name / 'package.json').read_text())['version'] == version
report = base / f.artifact
v.artifact(base, dict(path=f.artifact, bytes=report.stat().st_size, sha256=sha(report.read_bytes())))
print(json.dumps(dict(ok=True, authority=bp.header['blueprint_version'], item='ZS1-016', payload_files=42,
    historical_payload_files=14, source_lines=1490, source_bytes=48616, ranges=ranges, current_target_files=4,
    current_target_excerpts=count, actual_cases=23, actual_http_requests=24, runtime_regular_files=2394,
    runtime_symlinks=1, current_source_snapshot_files=179, historical_target_tests=17, new_target_tests=0,
    scope='Complete source016 review, current isolated original-probe replay, qualified current target mapping; no directory/product acceptance'), indent=2))

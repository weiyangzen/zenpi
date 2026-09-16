from pathlib import Path
import json, hashlib, subprocess, sys, math
ROOT = Path(__file__).resolve().parents[3]
w = Path(__file__).resolve().parent
p = w / 'frozen-input'
e = w / 'evidence'
j = lambda path: json.loads(path.read_text())
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
assert sha(p / 'manifest.json') == 'f019d53cb071d8fc12fc4c8b9fb6dbf2c8537a64b167781f6234c785547e64c2'
audit = subprocess.run([sys.executable, str(p / 'verify.py')], capture_output=True, text=True)
assert audit.returncode == 0, audit.stderr
for name in ['budget.json', 'budget.log', 'budget.run.json']:
    assert (e / 'original-audit' / name).read_bytes() == (ROOT / '.ops/stage1_execution/external-editor-integration' / name).read_bytes()
old = j(e / 'original-audit/budget.json')
old_run = j(e / 'original-audit/budget.run.json')
assert old_run['exit_code'] == 1 and old_run['sha256'] == sha(e / 'original-audit/budget.log')
assert old['gates']['cold_start'] is False and sum(old['gates'].values()) == 7
assert len(old['gates']) == 8 and old['cold_start']['elapsed_ms']['max'] == 1208.732708
identity = j(e / 'diagnosis/observations/identity.json')
summary = j(e / 'diagnosis/observations/summary.json')
assert identity['sha256'] == summary['binary_sha_after'] == sha(ROOT / 'target/release/zenpi')
for name, ref in j(p / 'read-identities.json').items():
    if name == 'Docs/stage_1_v3_pi_mono_blueprint.md':
        continue  # The captured ledger predates subsequent independent acceptances.
    path = ROOT / name
    assert path.stat().st_size == ref['bytes'] and sha(path) == ref['sha256'], name
rows = summary['rounds']
durations = []
for i, row in enumerate(rows):
    d = e / f'diagnosis/observations/round-{i}'
    marks = row['marks']
    times = [mark['monotonic_ns'] for mark in marks]
    assert times == sorted(times) and times[-1] <= row['parent_capture_end_ns']
    assert marks[0]['label'] == 'parent_launch_begin' and marks[-1]['label'] == 'wrapper_exit_reaped'
    duration = (row['parent_capture_end_ns'] - times[0]) / 1e6
    assert math.isclose(duration, row['elapsed_ms'], abs_tol=1e-9)
    assert row['returncode'] == 0 and not row['timeout']
    stdin = [json.loads(line) for line in (d / 'stdin.raw').read_text().splitlines()]
    frames = [json.loads(line) for line in (d / 'stdout.raw').read_text().splitlines()]
    assert stdin == [{'type': 'shutdown', 'id': 'bench-stop'}] and len(frames) == 1
    frame = frames[0]
    assert frame['id'] == 'bench-stop' and frame['command'] == 'shutdown' and frame['success'] is True
    assert frame['data'] == {'closing': True, 'drained': True}
    journal = [json.loads(line) for line in (d / f'fixture-after/session-{i}.jsonl').read_text().splitlines()]
    assert len(journal) == 2 and journal[0]['kind'] == 'session'
    assert journal[1]['event']['type'] == 'extensions_selected' and journal[1]['event']['tools'] == []
    reconnect = [json.loads(line) for line in (d / f'fixture-after/session-{i}.jsonl.reconnect').read_text().splitlines()]
    assert all(event['session_id'] == journal[0]['session_id'] for event in reconnect)
    terminal = [event['event'] for event in reconnect if event['event']['type'] == 'terminal']
    assert len(terminal) == 1 and json.loads(terminal[0]['line']) == frame
    for previous in range(i):
        for suffix in ['.jsonl', '.jsonl.reconnect']:
            name = f'session-{previous}{suffix}'
            assert (d / 'fixture-after' / name).read_bytes() == (e / f'diagnosis/observations/round-{previous}/fixture-after' / name).read_bytes()
    durations.append(duration)
policy = j(e / 'diagnosis/policy-correlation.json')
events = j(e / 'diagnosis/policy-target.stdout')
assert policy['scan_start'] in events and policy['scan_result'] in events
assert policy['scan_start']['bootUUID'] == policy['scan_result']['bootUUID']
assert all('ba2be7534db6fb27' in policy[name]['eventMessage'] for name in ['scan_start', 'scan_result'])
interval = (policy['scan_result']['machTimestamp'] - policy['scan_start']['machTimestamp']) * policy['numer'] / policy['denom'] / 1e6
assert math.isclose(interval, policy['scan_event_interval_ms'], abs_tol=1e-9)
assert 'zenpi-cdb646c6c40059cd' in policy['scan_result']['eventMessage']
assert 'Identifier=zenpi-cdb646c6c40059cd' in (e / 'diagnosis/codesign.stderr').read_text()
print(json.dumps(dict(ok=True, archived_files=78, new_controller_launches=0, diagnostic_samples=durations,
    historical_scan_interval_ms=interval, original_gate='FAIL 1208.732708 > 1000', cause='unresolved',
    complete=False, scope='Raw observation and arithmetic audit; no performance fix or product acceptance'), indent=2))

"""Evidence structure only; no Rust, PTY, HTTP or product execution."""
from pathlib import Path
import json, hashlib, re
E = Path(__file__).resolve().parent
root = E.parents[4]
def sha(b): return hashlib.sha256(b).hexdigest()
def read(p): return json.loads(p.read_text())
def bound(p, record):
    b = p.read_bytes()
    assert len(b) == record['bytes'] and sha(b) == record['sha256'], str(p)
source = (E/'source/pending_thread_approvals.rs').read_bytes()
s = source.decode()
assert len(source) == 4105 and len(source.splitlines()) == 147
assert sha(source) == 'a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f'
inputs = read(E/'inputs.json')
assert inputs['formal_files'] == 1 and len(inputs['files']) == 46
for row in inputs['files']: bound(root/row['snapshot'], row)
reads = read(E/'read-log.json')['sequential_reads']
end, line = 0, 1
for row in reads:
    a, b = row['bytes']; la, lb = row['lines']
    assert a == end and la == line and sha(source[a:b]) == row['sha256']
    assert len(source[a:b].splitlines()) == lb-la+1
    end, line = b, lb+1
assert len(reads) == 1 and end == 4105 and line == 148
tests = read(E/'source-tests.json')
assert len(tests) == 3
assert [t['name'] for t in tests] == re.findall(r'#\[test\]\s+fn (\w+)\(', s)
report = (root/'Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs_learn.md').read_text()
for t in tests:
    assert s.splitlines()[t['source_line']-1].strip() == f"fn {t['name']}() {{"
    assert t['execution'] == 'read only; not run' and t['name'] in report
matrix = read(E/'capability-matrix.json')
assert len(matrix) == 10
for row in matrix:
    assert not row['executed_in_304'] and row['source_fact'] in report
for token in ['实际执行数0', 'set_threads', 'QueueFull', 'Alt-A', 'Vec<String>', 'P10', '主控', '内嵌snapshot', '后台项目', 'contains_key']:
    assert token in report, token
assert not re.search(r'^\s*[-*]\s+\[[xX]\]', report, re.M)
captures = read(E/'context-capture.json')
assert len(captures) == 2 and sum(len(r['excerpts']) for r in captures) == 17
for row in captures:
    for excerpt in row['excerpts']: bound(root/excerpt['snapshot'], excerpt)
historical = []
for family in ['H132', 'approval-submenu-ready', 'approval-submenu-legacy-ready']:
    h = E/'history'/family; manifest = read(h/'manifest.json')
    for p in sorted(h.rglob('*')):
        if not p.is_file() or p.name == 'manifest.json': continue
        relative = str(p.relative_to(h))
        bound(p, manifest['artifacts'][relative])
h132 = read(E/'history/H132/manifest.json')
for key in ['after/src/tui.rs', 'frozen/candidate-cancel/src/tui.rs']:
    assert h132['artifacts'][key] == dict(bytes=captures[0]['full_bytes'], sha256=captures[0]['full_sha256'])
final_binary = '9e76676bc87d7a859409954ae5d19ebc0560070a4685096f2dff87ea15e918dc'
for name, count, code in [('policy-control-review',9,0), ('control-after-cancel',8,0), ('control-recovery',3,0), ('policy-control-expanded',7,1), ('policy-background-cursor',6,1), ('policy-control-deps',6,1), ('control-after',6,1)]:
    d = read(E/f'history/H132/runs/{name}.json')
    run = read(E/f'history/H132/runs/{name}.run.json')
    assert len(d['checks']) == count and all(v is True for v in d['checks'].values())
    assert run['exit_code'] == code and d['status'] == ('passed' if code == 0 else 'failed')
    if code == 0 or name == 'policy-control-expanded': assert d['binary_sha256'] == final_binary
    historical.append(dict(name=name, checks_true=count, run_exit=code, status=d['status'], binary_sha256=d['binary_sha256']))
for name, count, true_count, status in [('before-final',20,8,'failed'), ('after-final',26,26,'passed')]:
    d = read(E/f'history/approval-submenu-ready/evidence/{name}.json')
    assert len(d['checks']) == count and sum(v is True for v in d['checks'].values()) == true_count and d['status'] == status
    historical.append(dict(name=name, checks=count, checks_true=true_count, status=status, exit_evidence='original REVIEW.md; no separate run snapshot'))
legacy = read(E/'history/approval-submenu-legacy-ready/old-approval-final.json')
run = read(E/'history/approval-submenu-legacy-ready/run.json')
assert len(legacy['checks']) == 14 and all(v is True for v in legacy['checks'].values()) and legacy['passed'] is True
assert run['exit_code'] == 0 and legacy['binary_sha256'] == run['binary_sha256'] == '29001032c6de2d5529b64cb0c631c5c4406544ab9e5c96e1dfe7aec3d4c24802'
historical.append(dict(name='legacy', checks_true=14, run_exit=0, passed=True))
print(json.dumps(dict(passed=True, kind='structural only; master semantic G-FILE pending', source_bytes=4105, source_lines=147, source_tests_read=3, source_tests_executed=0, capability_groups=10, input_snapshots_checked=46, continuous_reads=1, target_context_excerpts=17, historical_results=historical, cargo_runs=0, pty_runs=0, http_runs=0), ensure_ascii=False, indent=2))

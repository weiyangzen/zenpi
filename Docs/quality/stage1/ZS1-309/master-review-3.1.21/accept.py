from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-309'
D = R / '.ops/stage1_execution/reference309-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-309/master-review-3.1.21'
assert not Q.exists(), 'one-shot publication already exists'

def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, data): p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '753b78de489fb016ed4f1d5f7ef4d60a852cd5bc78357a3b2ad1d982adbecd05'
audit = read(D / 'offline.stdout.json')
assert audit['checks'] == 136 and audit['failed'] == 0
for name, row in read(W / 'manifest.json')['files'].items():
    assert meta(W / name) == {k: row[k] for k in ['bytes', 'sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
f = bp.files[KEY]
report = f.artifact
assert bp.items[KEY].state == '[ ]' and bp.items[KEY].depends == ('ZS1-001',)
assert bp.items['ZS1-001'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / 'files' / report
assert meta(candidate) == dict(bytes=23290, sha256='97dd06e7a5f49a84558b0119b6e76a6f055f83313ec4110edd2ef9b6e4c05d6f')
source = (W / 'evidence/capture/codex/codex-rs/tui/src/status_indicator_widget.rs').read_bytes()
assert source == (Path(bp.scopes[f.scope][1]) / f.path).read_bytes()
ranges = [[0, len(source)]]
v.check_ranges(ranges, len(source))
current = read(D / 'current-inputs.json')
for row in current:
    assert meta(Path(row['origin'])) == row['current'], row['origin']
assert (R / 'src/tui.rs').read_bytes() == (W / 'evidence/freeze-capture/capture/zenpi/src/tui.rs').read_bytes()
append = '\n\n' + (D / 'root-review.md').read_text()
upstream = Path(bp.header['source_repo'])
sizes = v.file_hashes(bp, R, upstream, target=False)
assert v.validate(R)['ok']

Q.mkdir(parents=True)
shutil.copy2(D / 'root-review.md', Q / 'review.md')
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['offline.stdout.json', 'offline.stderr.log', 'offline.run.json', 'current-inputs.json', 'src__tui.rs.patch', 'current-target-read-bindings.json']:
    shutil.copy2(D / name, Q / name)
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file(): archive.add(p, arcname=p.relative_to(W).as_posix(), recursive=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    dest = Q / 'authority-before' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(R / rel, dest)
(R / report).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / report, candidate.read_text() + append)
refs = [record(R / report)] + [record(Q / name) for name in ['review.md', 'worker-report.md', 'worker-manifest.json',
    'worker-ready.tar.gz', 'offline.stdout.json', 'current-inputs.json', 'offline.run.json', 'src__tui.rs.patch', 'current-target-read-bindings.json', 'accept.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='reference309-current-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, source_path=f.path,
    source_hash=f.sha256, read_ranges=ranges, artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker C with independent controller semantic review and current target delta')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Complete309 source and candidate independently read; bounded callers, 7 source tests, 3 target tests and 19 current target fragments reviewed.136 fresh offline checks pass. Timer, cancellation, display and resource boundaries retained; only309 accepted, no product or platform acceptance.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-309\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
    assert count == 1
    updated = v.parse(text)
    assert updated.requirement == bp.requirement
    g.atomic(R / v.BLUEPRINT, text)
    active = read(R / v.SELECTOR)
    active['snapshot_sha256'] = updated.snapshot
    g.atomic(R / v.SELECTOR, json.dumps(active, ensure_ascii=False, separators=(',', ':')) + '\n')
    for rel, (fields, rows) in v.scaffold(updated, sizes, v.EVIDENCE).items(): g.atomic(R / rel, v.tsv(fields, rows).decode())
    front = v.frontiers(updated, read(R / v.EVIDENCE / 'claims.json')['claims'], sel['run_id'], 3)
    for todo in (R / v.EVIDENCE).glob('todos_*.md'): g.atomic(todo, v.todo(updated, front, v.BLUEPRINT, v.EVIDENCE + '/claims.json'))

state('[_]')
pre = v.validate(R, item=KEY)
dump(Q / 'before-promotion.json', pre)
assert pre['ok'], pre
state('[x]')
post = v.validate(R, item=KEY)
dump(Q / 'after-promotion.json', post)
assert post['ok'], post
argv = ['python3', 'tools/validate_stage1_blueprint.py', '--item', KEY]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.run(argv, cwd=R, capture_output=True, timeout=60)
(Q / 'gstage.stdout.log').write_bytes(proc.stdout)
(Q / 'gstage.stderr.log').write_bytes(proc.stderr)
dump(Q / 'gstage.run.json', dict(argv=argv, cwd=str(R), started_at=started,
    ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=proc.returncode,
    stdout=meta(Q / 'gstage.stdout.log'), stderr=meta(Q / 'gstage.stderr.log')))
assert proc.returncode == 0, proc.stderr
dump(Q / 'manifest.json', dict(item=KEY, master_accepted=True, artifacts={p.relative_to(Q).as_posix(): meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main()
print(json.dumps(dict(accepted=KEY, counts=post['counts'], snapshot=post['snapshot_sha256'])))

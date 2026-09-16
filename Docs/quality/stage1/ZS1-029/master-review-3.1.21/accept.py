from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-029'
D = R / '.ops/stage1_execution/source029-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-029/master-review-3.1.21'
assert not Q.exists(), 'one-shot publication already exists'

def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, data): p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '70c63be72d4fd8c2d69b03d679bf28a5ad8a49c52be26651ca6cb029218a7322'
audit = read(D / 'worker-offline.json')
assert audit['passed'] and len(audit['checks']) == 55
for row in read(W / 'manifest.json')['files']:
    assert meta(W / row['path']) == {k: row[k] for k in ['bytes', 'sha256']}
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
assert meta(candidate)['sha256'] == '254d3041e58ad6aa5d17587d8e9b4dad3509341ba39922f17d457ee49797c21a'
assert candidate.read_bytes() == (W / 'report-body.md').read_bytes()
current = {}
for row in read(W / 'input-inventory.json'):
    if row['kind'] == 'authority':
        continue
    p = Path(row['origin'])
    expected = {k: row[k] for k in ['bytes', 'sha256']}
    actual = meta(p)
    if row['path'] == 'src/headless.rs':
        assert expected['sha256'] == '521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0'
        assert actual == dict(bytes=370207, sha256='bf252fec95a859b26f9aea402c918e9efbf6c2c55d3e24b403831f86d80ec06e')
    else:
        assert actual == expected, row['path']
    current[row['origin']] = dict(captured=expected, current=actual)

prior = R / '.ops/stage1_execution/source017-current-3.1.21/worker'
reuses = []
for row in read(W / 'reused-context-inventory.json'):
    p = prior / row['prior_artifact']
    q = W / row['current_artifact']
    assert p.read_bytes() == q.read_bytes()
    reuses.append(dict(prior=record(p), current=record(q)))
contexts = []
for row in read(W / 'context-reading-ledger.json'):
    rel = row['path']
    if rel.startswith('target/'):
        p = R / rel.removeprefix('target/')
    elif rel.startswith('source/'):
        p = Path(bp.header['source_repo']) / rel.removeprefix('source/')
    else:
        continue
    lo, hi = row['line_range']
    b = b''.join(p.read_bytes().splitlines(True)[lo-1:hi])
    assert b == (W / row['excerpt']).read_bytes(), rel
    contexts.append(dict(path=str(p), line_range=[lo, hi], excerpt_sha256=hashlib.sha256(b).hexdigest()))
headless = b''.join((R / 'src/headless.rs').read_bytes().splitlines(True)[2609:2688])
assert headless == (W / 'reused-context/context-excerpts/src__headless.rs-2610-2688.txt').read_bytes()
source = (W / 'source' / f.path).read_bytes()
ranges = [row['byte_range'] for row in read(W / 'reading-ledger.json')]
v.check_ranges(ranges, len(source))
for row in read(W / 'reading-ledger.json'):
    lo, hi = row['byte_range']
    assert source[lo:hi] == (W / row['excerpt']).read_bytes()
append = '\n\n' + (D / 'root-review.md').read_text()
upstream = Path(bp.header['source_repo'])
sizes = v.file_hashes(bp, R, upstream, target=False)
assert v.validate(R)['ok']

Q.mkdir(parents=True)
shutil.copy2(D / 'root-review.md', Q / 'review.md')
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['worker-offline.json', 'worker-offline.stderr', 'current-input-delta.json', 'headless-delta.patch']:
    shutil.copy2(D / name, Q / name)
dump(Q / 'current-inputs.json', dict(inputs=current, reused_root017=reuses, current_contexts=contexts,
    headless_current_excerpt_sha256=hashlib.sha256(headless).hexdigest(),
    offline_execution='Retained preceding-turn one-time package-only verifier output; not rerun during publication. No reconstructed timing claim.'))
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
    'worker-ready.tar.gz', 'worker-offline.json', 'current-inputs.json', 'current-input-delta.json', 'headless-delta.patch', 'accept.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='source029-current-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, source_path=f.path,
    source_hash=f.sha256, read_ranges=ranges, artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker B with frozen C evidence and independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Complete029 source and candidate independently read, Chat fold and bounded production mapping reviewed; actual historical33-case/37HTTP probe read. Preserve strictness/usage/compat differences, frozen historical failures, current headless delta and exact017 context reuse. Only029 accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-029\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

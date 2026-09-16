from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-021'
D = R / '.ops/stage1_execution/source021-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-021/master-review-3.1.21'
assert not Q.exists(), 'one-shot publication already exists'

def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, data): p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '624213df1e2b5ad2d9f943440d55c11951c04e38b6d39f714c8cc687479d4ea4'
audit = read(D / 'worker-offline.json')
assert audit['passed'] and len(audit['checks']) == 1198
for row in read(W / 'manifest.json')['payload']:
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
candidate = W / report
assert meta(candidate)['sha256'] == '3b539b2388a6cb73e2d8b1275aa5529d6d0aa54720fa8fd2761e53df93738068'
current = []
for row in read(W / 'input-inventory.json')['target']:
    actual = meta(R / row['path'])
    expected = {k: row[k] for k in ['bytes', 'sha256']}
    assert actual == expected, row['path']
    current.append(dict(path=row['path'], captured=expected, current=actual))
source = (W / 'source/resource-loader.ts').read_bytes()
assert source == (Path(bp.header['source_repo']) / f.path).read_bytes()
source_rows = [row for row in read(W / 'reading-ranges.json')['ranges'] if row['id'].startswith('S')]
ranges = [[row['start_byte'], row['end_byte']] for row in source_rows]
v.check_ranges(ranges, len(source))
for row in source_rows:
    assert source[row['start_byte']:row['end_byte']] == (W / row['excerpt']).read_bytes()
contexts = []
for row in read(W / 'reading-ranges.json')['ranges']:
    if row['id'] not in ['T1', 'T2', 'T3', 'T4', 'T5', 'T6', 'T7', 'T8']:
        continue
    path = R / row['snapshot'].removeprefix('target/')
    frag = b''.join(path.read_bytes().splitlines(True)[row['start_line']-1:row['end_line']])
    assert frag == (W / row['excerpt']).read_bytes()
    contexts.append(dict(path=str(path), line_range=[row['start_line'], row['end_line']], sha256=hashlib.sha256(frag).hexdigest()))
reuse = []
for row in read(W / 'source-identity-reuse.json'):
    path = Path(row['original_path'])
    assert meta(path) == {k: row[k] for k in ['bytes', 'sha256']}
    assert path.read_bytes() == (W / row['snapshot']).read_bytes()
    reuse.append(row)
append = '\n\n' + (D / 'root-review.md').read_text()
upstream = Path(bp.header['source_repo'])
sizes = v.file_hashes(bp, R, upstream, target=False)
assert v.validate(R)['ok']

Q.mkdir(parents=True)
shutil.copy2(D / 'root-review.md', Q / 'review.md')
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['worker-offline.json', 'worker-offline.stderr', 'worker-offline.run.json', 'current-input-audit.json']:
    shutil.copy2(D / name, Q / name)
dump(Q / 'current-inputs.json', dict(inputs=current, current_contexts=contexts,
    source_identity_reuse=read(W / 'source-identity-reuse.json'),
    offline_execution='One fresh read-only portable verifier invocation from /tmp; exact run record preserved. No old captured program execution.',
    root_reading='Full six source ranges, full candidate and probe, T1 through T8; T9 and old restored helper not claimed as root reads.'))
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
    'worker-ready.tar.gz', 'worker-offline.json', 'current-inputs.json', 'current-input-audit.json', 'worker-offline.run.json', 'accept.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='source021-current-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, source_path=f.path,
    source_hash=f.sha256, read_ranges=ranges, artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker B with frozen C probe and independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Complete021 source, full candidate and historical23-case probe independently read; target loader and bounded host mappings reviewed. Preserve four doubles, original failure logs, partial source reload and limited target publication guarantees. Only021 accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-021\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

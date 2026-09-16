from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-055'
D = R / '.ops/stage1_execution/directory055-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-055/master-review-3.1.21'
assert not Q.exists(), 'one-shot publication already exists'
def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, data): p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '466c784bf50cdb078b727085f66eae5fd8bdc15e955cf2437c00c89045a41492'
assert read(D / 'offline.run.json')['exit_code'] == 0
audit = read(D / 'offline.stdout.json')
assert audit['passed'] and len(audit['checks']) == 61
for row in read(W / 'manifest.json')['files']:
    assert meta(W / row['path']) == {k: row[k] for k in ['bytes', 'sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
children = list(bp.items[KEY].depends)
assert children == ['ZS1-016', 'ZS1-017', 'ZS1-029'] and bp.items[KEY].state == '[ ]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / 'files' / report
assert meta(candidate) == dict(bytes=27218, sha256='7d7d02a264f22b3364bae34ac726c19b09d4e6ad186c9ada13bfb82ac8e29cfe')
assert candidate.read_bytes() == (W / 'report-body.md').read_bytes()
upstream = Path(bp.header['source_repo'])
inventory = read(W / 'direct-inventory.json')
assert len(inventory) == 32 and {p.name for p in (upstream / folder).iterdir()} == {r['name'] for r in inventory}
for row in inventory:
    p = upstream / row['path']
    assert p.is_file() and not p.is_symlink()
    assert meta(p) == {k: row[k] for k in ['bytes', 'sha256']}
current = {}
for row in read(W / 'input-inventory.json'):
    actual = meta(Path(row['origin']))
    expected = {k: row[k] for k in ['bytes', 'sha256']}
    if row['path'] == 'target/src/headless.rs':
        assert expected['sha256'] == '521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0'
        assert actual == dict(bytes=370207, sha256='bf252fec95a859b26f9aea402c918e9efbf6c2c55d3e24b403831f86d80ec06e')
    else:
        assert actual == expected, row['path']
    current[row['origin']] = dict(captured=expected, current=actual)
reuses = []
for filename, olddir in [('simple-options.ts','source017-current-3.1.21'), ('transform-messages.ts','source029-current-3.1.21'),
                         ('constrained-sampling.ts','source029-current-3.1.21'), ('google-shared.ts','source017-current-3.1.21')]:
    p = R / '.ops/stage1_execution' / olddir / 'worker/source/packages/ai/src/api' / filename
    q = W / 'source/packages/ai/src/api' / filename
    assert p.read_bytes() == q.read_bytes()
    reuses.append(dict(prior=record(p), current=record(q)))
old = R / '.ops/stage1_execution/source017-current-3.1.21/worker/source/packages/ai/src/utils/event-stream.ts'
assert old.read_bytes() == (W / 'source/packages/ai/src/utils/event-stream.ts').read_bytes()
assert (W / 'dependency029-candidate/manifest.json').read_bytes() == (R / '.ops/stage1_execution/source029-current-3.1.21/worker/manifest.json').read_bytes()
sizes = v.file_hashes(bp, R, upstream, target=False)
dep_records = {}
for key in children:
    assert bp.items[key].state == '[x]'
    p = R / v.EVIDENCE / 'receipts' / f'{key}.master.json'
    v.check_receipt(R, bp, key, v.EVIDENCE, sel, sizes, 'master')
    dep_records[key] = record(p)
    if key != 'ZS1-029':
        assert p.read_bytes() == (W / ('dependency' + key[-3:]) / 'receipt.json').read_bytes()
assert v.validate(R)['ok']

Q.mkdir(parents=True)
for name in ['root-review.md', 'offline.stdout.json', 'offline.stderr.log', 'offline.run.json']:
    shutil.copy2(D / name, Q / ('review.md' if name == 'root-review.md' else name))
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
shutil.copy2(R / '.ops/stage1_execution/source029-current-3.1.21/headless-delta.patch', Q / 'headless-delta.patch')
dump(Q / 'current-inputs.json', dict(inputs=current, exact_prior_source_contexts=reuses, current_children=dep_records))
(Q / 'current-child-receipts').mkdir()
for key in children:
    shutil.copy2(R / v.EVIDENCE / 'receipts' / f'{key}.master.json', Q / 'current-child-receipts' / f'{key}.master.json')
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file(): archive.add(p, arcname=p.relative_to(W).as_posix(), recursive=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    dest = Q / 'authority-before' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(R / rel, dest)
(R / report).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / report, candidate.read_text() + '\n\n' + (D / 'root-review.md').read_text())
refs = [record(R / report)] + [record(Q / name) for name in ['review.md', 'worker-report.md', 'worker-manifest.json',
    'worker-ready.tar.gz', 'offline.stdout.json', 'offline.run.json', 'current-inputs.json', 'headless-delta.patch', 'accept.py']]
refs += [record(p) for p in sorted((Q / 'current-child-receipts').iterdir())]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='directory055-current-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope,
    folder_path=folder, children=children, artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker B with historical C evidence and current controller dependency qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Independent055 directory integration review:32 direct entries,3 individually accepted children; actual lazy/dispatch/reference/terminal contracts and historical9-case/3HTTP probe read.61 new integrity checks pass. Current headless delta qualified; no automatic parent, context-file or product parity acceptance.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-055\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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
pre = v.validate(R, item=KEY); dump(Q / 'before-promotion.json', pre); assert pre['ok'], pre
state('[x]')
post = v.validate(R, item=KEY); dump(Q / 'after-promotion.json', post); assert post['ok'], post
argv = ['python3', 'tools/validate_stage1_blueprint.py', '--item', KEY]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.run(argv, cwd=R, capture_output=True, timeout=60)
(Q / 'gstage.stdout.log').write_bytes(proc.stdout); (Q / 'gstage.stderr.log').write_bytes(proc.stderr)
dump(Q / 'gstage.run.json', dict(argv=argv, cwd=str(R), started_at=started,
    ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=proc.returncode,
    stdout=meta(Q / 'gstage.stdout.log'), stderr=meta(Q / 'gstage.stderr.log')))
assert proc.returncode == 0, proc.stderr
dump(Q / 'manifest.json', dict(item=KEY, master_accepted=True, artifacts={p.relative_to(Q).as_posix(): meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main()
print(json.dumps(dict(accepted=KEY, counts=post['counts'], snapshot=post['snapshot_sha256'])))

from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-061'
D = R / '.ops/stage1_execution/directory061-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-061/master-review-3.1.21'
assert not Q.exists(), 'one-shot publication already exists'
def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, data): p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '2bb37eeee4b41724903ee4d5f5bc6b632991588327a615465460dee4fd4954ff'
assert read(D / 'root-offline.run.json')['exit_code'] == 0
audit = read(D / 'root-offline.stdout.json')
assert audit['ok'] and audit['checks'] == 62
for row in read(W / 'manifest.json')['files']:
    assert meta(W / row['path']) == {k: row[k] for k in ['bytes', 'sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
children = list(bp.items[KEY].depends)
assert children == ['ZS1-057', 'ZS1-058'] and bp.items[KEY].state == '[ ]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / 'report' / report
assert meta(candidate) == dict(bytes=22581, sha256='18635eef7236bbfd1f1420bcb05fe01e7f0536549c8a036dace1df27620d3352')
upstream = Path(bp.header['source_repo'])
inventory = read(W / 'direct-inventory.json')
assert len(inventory) == 12 and {p.name for p in (upstream / folder).iterdir()} == {r['name'] for r in inventory}
for row in inventory:
    p = upstream / folder / row['name']
    assert not p.is_symlink()
    if row['kind'] == 'file':
        assert p.is_file() and meta(p) == {k: row[k] for k in ['bytes', 'sha256']}
    else:
        assert p.is_dir() and {q.name for q in p.iterdir()} == {x['name'] for x in row['direct_children']}
        for child in row['direct_children']:
            q = p / child['name']
            assert not q.is_symlink()
            if child['kind'] == 'file':
                assert q.is_file() and meta(q) == {k: child[k] for k in ['bytes', 'sha256']}
            else:
                assert q.is_dir()
current = []
for name in ['reading-ledger.json']:
    for row in read(W / name):
        actual = meta(upstream / row['path'])
        assert actual == {k: row[k] for k in ['bytes', 'sha256']}, row['path']
        current.append(dict(path=row['path'], **actual, evidence=name))
for observation in read(D / 'root-current-inputs.json')['inputs']:
    assert meta(Path(observation['path'])) == {k: observation[k] for k in ['bytes', 'sha256']}, observation['path']
assert read(D / 'root-reading-progress.json')['status'] == 'independent-review-complete-ready-for-acceptance'
sizes = v.file_hashes(bp, R, upstream, target=False)
dep_records = {}
for key in children:
    assert bp.items[key].state == '[x]'
    p = R / v.EVIDENCE / 'receipts' / f'{key}.master.json'
    v.check_receipt(R, bp, key, v.EVIDENCE, sel, sizes, 'master')
    dep_records[key] = record(p)
    assert p.read_bytes() == (W / 'accepted' / v.EVIDENCE / 'receipts' / f'{key}.master.json').read_bytes()
assert v.validate(R)['ok']

Q.mkdir(parents=True)
for name in ['root-review.md', 'root-offline.stdout.json', 'root-offline.stderr.txt', 'root-offline.run.json', 'root-current-inputs.json', 'root-reading-progress.json']:
    shutil.copy2(D / name, Q / ('review.md' if name == 'root-review.md' else name))
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
dump(Q / 'current-inputs.json', dict(inputs=current, direct_inventory=inventory, current_children=dep_records))
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
    'worker-ready.tar.gz', 'root-offline.stdout.json', 'root-offline.run.json', 'current-inputs.json', 'root-current-inputs.json', 'root-reading-progress.json', 'accept.py']]
refs += [record(p) for p in sorted((Q / 'current-child-receipts').iterdir())]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='directory061-current-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope,
    folder_path=folder, children=children, artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker A with independently reviewed current child acceptance chains')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Independent061 package review:7 files/5 dirs, accepted children057/058;28 full context reads3603lines and three historical child observations independently read;62 fresh offline checks pass;140 current identity records exact. Explicit summary converter and process-local versus durable state qualified. No package-build, parent or product acceptance.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-061\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

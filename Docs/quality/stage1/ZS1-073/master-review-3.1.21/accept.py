from pathlib import Path
import datetime, difflib, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-073'
D = R / '.ops/stage1_execution/target073-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-073/master-review-3.1.21'
assert not Q.exists()

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def read(p):
    return json.loads(p.read_text())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

def rec(p):
    return dict(path=str(p.relative_to(R)), **meta(p))

bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
f = bp.files[KEY]
assert bp.requirement == '3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d'
assert sum(i.state == '[x]' for i in bp.items.values()) == 51
assert bp.items[KEY].state == '[ ]'
assert all(bp.items[k].state == '[x]' for k in bp.items[KEY].depends)
assert (R / f.artifact).read_bytes() == (W / 'receiver-base.md').read_bytes()
assert meta(R / f.artifact) == dict(bytes=3036, sha256='08f9dc2204499b1431e049bb6e42c5677a24eb4934f4c3672cf3f17063b5219b')
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
assert meta(W / 'manifest.json')['sha256'] == '63f737ad072cd8454571c2306c79b540820d19751531cdaabb19bd019cedd5c1'
assert meta(W / 'learn-report.md')['sha256'] == '5f6325ffe2286403f5926605aef66cf16bdb9e104f4a7aa1be9b2bdd4c1c6e2b'
source = R / f.path
baseline = W / 'target/baseline-context.rs'
assert source.read_bytes() == (W / 'target/src/context.rs').read_bytes()
assert meta(source) == dict(bytes=30013, sha256='256d9ac1657e272ba61ebfbc242ece2970ed0607f3ebb1583b1711f18b7bf229')
assert meta(baseline) == dict(bytes=8199, sha256=f.sha256)
assert f.sha256 == 'bc4ce7999a0f4d162effece8c6f839aca3a230e97675a1d7908fca5d6a7fe909'
new = source.read_bytes().splitlines(keepends=True)
assert len(new) == 807 and len(baseline.read_bytes().splitlines()) == 246
run = read(D / 'root-offline/run.json')
audit = read(D / 'root-offline/stdout.log')
assert run['exit_code'] == 0 and audit['ok']
assert audit['result']['passed'] == 29 and audit['result']['failed'] == 0 and audit['result']['new_product_executions'] == 0
review = (D / 'root-review.md').read_text()

Q.mkdir(parents=True)
(Q / 'review.md').write_text(review)
shutil.copyfile(W / 'learn-report.md', Q / 'worker-report.md')
shutil.copyfile(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copyfile(source, Q / 'current-context.rs')
shutil.copyfile(baseline, Q / 'baseline-context.rs')
shutil.copyfile(Path(__file__), Q / 'accept.py')
shutil.copyfile(W / 'receiver-base.md', Q / 'receiver-base.md')
shutil.copyfile(D / 'root-offline/audit.py', Q / 'root-audit.py')
for n in ['stdout.log', 'stderr.log', 'run.json']:
    shutil.copyfile(D / 'root-offline' / n, Q / ('offline-' + n))
ranges = [[1,280],[281,560],[561,807]]
dump(Q / 'read-binding.json', dict(
    formal_source=rec(Q / 'current-context.rs'), current_line_ranges=ranges,
    current_byte_ranges=[[sum(map(len,new[:a-1])),sum(map(len,new[:b]))] for a,b in ranges],
    baseline=rec(Q / 'baseline-context.rs'), identical_to_current=False, baseline_full_read_range=[1,246],
    source_read_tool_chunks=['3741ab','5bad3a','9b1233'],
    report=rec(Q / 'worker-report.md'), product_executions=0))
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file():
            archive.add(p, arcname=str(p.relative_to(W)), recursive=False)
sizes = v.file_hashes(bp, R, Path(bp.header['source_repo']), target=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    dest = Q / 'authority-before' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(R / rel, dest)
(R / f.artifact).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / f.artifact, (W / 'learn-report.md').read_text() + '\n\n' + review)
refs = [rec(R / f.artifact)] + [rec(Q / n) for n in [
    'review.md','worker-report.md','worker-manifest.json','current-context.rs',
    'baseline-context.rs','read-binding.json','accept.py','worker-ready.tar.gz',
    'offline-stdout.log','offline-run.json','receiver-base.md','root-audit.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'],
    requirement_digest=bp.requirement, baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],
    complete=True, attempt_id='target073-current-independent-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators,
    source_path=f.path, source_hash=f.sha256, read_ranges=[[0,8199]], artifacts=refs)
for role in ['worker','master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker A with controller current qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=rec(Q/'review.md'),
            findings='Complete807-line current context,246-line frozen baseline,133-line report and1497 lines of supporting tests independently read; full core compaction and session checkpoint consumer paths read within bounded scope.29 new structural checks pass,0 product runs; only073 file understanding accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-073\*\*)', '- '+mark+r'\1', (R/v.BLUEPRINT).read_text(), flags=re.M)
    assert count == 1
    updated = v.parse(text)
    assert updated.requirement == bp.requirement
    g.atomic(R/v.BLUEPRINT,text)
    active = read(R/v.SELECTOR)
    active['snapshot_sha256'] = updated.snapshot
    g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():
        g.atomic(R/rel,v.tsv(fields,rows).decode())
    front = v.frontiers(updated,read(R/v.EVIDENCE/'claims.json')['claims'],sel['run_id'],3)
    for p in (R/v.EVIDENCE).glob('todos_*.md'):
        g.atomic(p,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))

state('[_]')
pre = v.validate(R,item=KEY)
dump(Q/'before-promotion.json',pre)
assert pre['ok'],pre
state('[x]')
post = v.validate(R,item=KEY)
dump(Q/'after-promotion.json',post)
assert post['ok'],post
argv = ['python3','tools/validate_stage1_blueprint.py','--item',KEY]
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
result = subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(result.stdout)
(Q/'gstage.stderr.log').write_bytes(result.stderr)
dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=result.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')))
assert result.returncode == 0,result.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main()
print(json.dumps(dict(accepted=KEY,counts=post['counts'],snapshot=post['snapshot_sha256'])))

from pathlib import Path
import datetime, difflib, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-086'
D = R / '.ops/stage1_execution/target086-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-086/master-review-3.1.21'
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
assert sum(i.state == '[x]' for i in bp.items.values()) == 52
assert bp.items[KEY].state == '[ ]'
assert all(bp.items[k].state == '[x]' for k in bp.items[KEY].depends)
assert not (R / f.artifact).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
assert meta(W / 'manifest.json')['sha256'] == '2c0cf1de5493ddb4ca342d389cce247fed3c64a91913cb8ca23abe1b1c9a1b8f'
assert meta(W / 'files' / f.artifact)['sha256'] == 'c70cf7abc9c282d81e55a1c696f01a94073f17eab98ed6d9641c66cd1e7d3be5'
source = R / f.path
baseline = W / read(W / 'baseline-comparison.json')['baseline_path']
assert source.read_bytes() == (W / 'capture/zenpi/src/view_model.rs').read_bytes()
assert meta(source) == dict(bytes=42009, sha256='094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe')
assert meta(baseline) == dict(bytes=40591, sha256=f.sha256)
assert f.sha256 == '1089a9f352b5aaebc755b51ece8189c1daaca1aa82060d2f009630b915b3abee'
new = source.read_bytes().splitlines(keepends=True)
assert len(new) == 1255 and len(baseline.read_bytes().splitlines()) == 1220
run = read(D / 'root-offline/run.json')
audit = [json.loads(line) for line in (D / 'root-offline/stdout.log').read_text().splitlines()]
assert run['exit_code'] == 0 and audit[-1] == dict(checks=154,failed=0,structural_only=True,runtime_execution=False)
assert all(row['passed'] for row in audit[:-1])
assert not (D / 'root-offline/stderr.log').read_bytes()
assert (R/'tests/view_model.rs').read_bytes() == (W/'capture/zenpi/tests/view_model.rs').read_bytes()
review = (D / 'root-review.md').read_text()

Q.mkdir(parents=True)
(Q / 'review.md').write_text(review)
shutil.copyfile(W / 'files' / f.artifact, Q / 'worker-report.md')
shutil.copyfile(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copyfile(source, Q / 'current-view_model.rs')
shutil.copyfile(baseline, Q / 'baseline-view_model.rs')
shutil.copyfile(Path(__file__), Q / 'accept.py')
shutil.copyfile(W / 'baseline-to-current.diff', Q / 'baseline-to-current.diff')
shutil.copyfile(R/'tests/view_model.rs',Q/'view_model-tests.rs')
shutil.copyfile(D / 'root-offline/audit.py', Q / 'root-audit.py')
for n in ['stdout.log', 'stderr.log', 'run.json']:
    shutil.copyfile(D / 'root-offline' / n, Q / ('offline-' + n))
ranges = [[1,300],[301,600],[601,900],[901,1255]]
dump(Q / 'read-binding.json', dict(
    formal_source=rec(Q / 'current-view_model.rs'), current_line_ranges=ranges,
    current_byte_ranges=[[sum(map(len,new[:a-1])),sum(map(len,new[:b]))] for a,b in ranges],
    baseline=rec(Q / 'baseline-view_model.rs'), identical_to_current=False, baseline_full_read_ranges=[[1,420],[421,840],[841,1220]],
    source_read_tool_chunks=['f72c1a','15d1be','f09cc5','fc96c6'],
    baseline_read_tool_chunks=['734df1','5578f9','ec7a55'],
    tests_read_tool_chunks=['f50a22','a8f292'],tests_read_line_ranges=[[1,225],[226,425]],
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
g.atomic(R / f.artifact, (W / 'files' / f.artifact).read_text() + '\n\n' + review)
refs = [rec(R / f.artifact)] + [rec(Q / n) for n in [
    'review.md','worker-report.md','worker-manifest.json','current-view_model.rs',
    'baseline-view_model.rs','read-binding.json','accept.py','worker-ready.tar.gz',
    'offline-stdout.log','offline-run.json','baseline-to-current.diff','view_model-tests.rs','root-audit.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'],
    requirement_digest=bp.requirement, baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],
    complete=True, attempt_id='target086-current-independent-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators,
    source_path=f.path, source_hash=f.sha256, read_ranges=[[0,40591]], artifacts=refs)
for role in ['worker','master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker C with controller current qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=rec(Q/'review.md'),
            findings='Complete1255-line current view_model,1220-line frozen baseline,245-line report and425-line external tests independently read; bounded TUI/headless/core event consumers read.154 fresh structural checks pass,0 product runs; only086 file understanding accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-086\*\*)', '- '+mark+r'\1', (R/v.BLUEPRINT).read_text(), flags=re.M)
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

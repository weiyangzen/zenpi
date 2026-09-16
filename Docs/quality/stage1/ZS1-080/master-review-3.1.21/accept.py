from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
KEY = 'ZS1-080'
D = R / '.ops/stage1_execution/target080-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-080/master-review-3.1.21'
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
assert sum(i.state == '[x]' for i in bp.items.values()) == 54
assert bp.items[KEY].state == '[ ]'
assert all(bp.items[k].state == '[x]' for k in bp.items[KEY].depends)
assert not (R / f.artifact).exists()
assert meta(W/'manifest.json')['sha256'] == '175d88faad7789422029f2a0b85c140f3a9248f260cff3aa115e2a26faef9dc1'
for role in ['worker','master']:
    assert not (R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json').exists()
source = R / f.path
baseline = W / 'target/baseline-extensions.rs'
assert source.read_bytes() == (W/'target/src/extensions.rs').read_bytes()
assert meta(source) == dict(bytes=20742,sha256='38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed')
assert meta(baseline) == dict(bytes=23071,sha256=f.sha256)
assert f.sha256 == '9b1879edcfb70e711960092e12060a9137e2aacda6bdc547318e5ccb7510fe39'
assert len(source.read_bytes().splitlines()) == 626
assert len(baseline.read_bytes().splitlines()) == 685
assert (R/'tests/extensions.rs').read_bytes() == (W/'context/full/tests/extensions.rs').read_bytes()
run = read(D/'root-offline/run.json')
audit = read(D/'root-offline/stdout.log')
assert run['exit_code'] == 0 and audit['ok'] and audit['result']['passed'] == 39 and audit['result']['failed'] == 0
assert all(row['passed'] for row in audit['checks'])
assert not (D/'root-offline/stderr.log').read_bytes()
assert meta(D/'root-offline/audit.py')['sha256'] == run['auditor_sha256']
review = (D/'root-review.md').read_text()
Q.mkdir(parents=True)
(Q/'review.md').write_text(review)
for src, name in [(W/'learn-report.md','worker-report.md'),(W/'manifest.json','worker-manifest.json'),
                  (source,'current-extensions.rs'),(baseline,'baseline-extensions.rs'),
                  (Path(__file__),'accept.py'),(W/'baseline-to-current.diff','baseline-to-current.diff'),
                  (R/'tests/extensions.rs','extensions-tests.rs'),(D/'root-offline/audit.py','root-audit.py')]:
    shutil.copyfile(src,Q/name)
for n in ['stdout.log','stderr.log','run.json','started.json']:
    shutil.copyfile(D/'root-offline'/n,Q/('offline-'+n))
ledger = read(W/'reading-ledger.json')
context = ledger['context_reads']
for e in context:
    assert meta(Path(e['original'])) == {k:e['whole_identity'][k] for k in ['bytes','sha256']}
dump(Q/'read-binding.json',dict(
    current=rec(Q/'current-extensions.rs'),current_ranges=[[1,315],[316,626]],current_tool_chunks=['2fb8d6','c793f1'],
    baseline=rec(Q/'baseline-extensions.rs'),baseline_ranges=[[1,345],[346,685]],baseline_tool_chunks=['ff44d1','5a3922'],
    report=rec(Q/'worker-report.md'),report_ranges=[[1,50],[51,100],[101,153]],report_tool_chunks=['2fb8d6','c793f1','40a2e7'],
    tests=rec(Q/'extensions-tests.rs'),tests_range=[1,180],tests_tool_chunk='40a2e7',
    runtime_prefix=dict(path='src/extension_runtime.rs',identity=meta(R/'src/extension_runtime.rs'),ranges=[[1,250],[251,440],[441,744]],tool_chunks=['4ec23e','ff44d1','5a3922'],full_file_acceptance=False),
    context_archive_path='worker-ready.tar.gz',context_worker_ledger=context,
    root_exact_archived_context_indices_zero_based=[5,6,9,10,11,12,13,14,15,16,17,19,20,21,22,23,24,25,26,27,29],
    root_context_tool_chunks=['c2bab8','758e99'],
    extra_current_context=dict(tool_chunk='cb471d',ranges={'src/core.rs':[[2108,2285],[5234,5278]],'src/tools.rs':[[1118,1207],[1336,1417]],'src/security.rs':[[410,430]]}),
    new_product_executions=0,root_auditor_static_read='43ff9d',root_auditor_single_run='cf07d4'))
with tarfile.open(Q/'worker-ready.tar.gz','x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file(): archive.add(p,arcname=str(p.relative_to(W)),recursive=False)
with tarfile.open(Q/'worker-external.tar.gz','x:gz') as archive:
    for p in sorted((D/'worker-external').iterdir()):
        assert p.is_file() and not p.is_symlink()
        archive.add(p,arcname=p.name,recursive=False)
sizes = v.file_hashes(bp,R,Path(bp.header['source_repo']),target=False)
for rel in [v.BLUEPRINT,v.SELECTOR,v.EVIDENCE+'/claims.json',*v.scaffold(bp,sizes,v.EVIDENCE)]:
    dest=Q/'authority-before'/rel
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(R/rel,dest)
(R/f.artifact).parent.mkdir(parents=True,exist_ok=True)
g.atomic(R/f.artifact,(W/'learn-report.md').read_text()+'\n\n'+review)
refs=[rec(R/f.artifact)]+[rec(Q/n) for n in ['review.md','worker-report.md','worker-manifest.json','current-extensions.rs','baseline-extensions.rs','extensions-tests.rs','read-binding.json','accept.py','worker-ready.tar.gz','worker-external.tar.gz','offline-stdout.log','offline-run.json','baseline-to-current.diff','root-audit.py']]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,
    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='target080-current-independent-3.1.21',
    integrated_revision=v.repository_head(R),validators=bp.items[KEY].validators,source_path=f.path,source_hash=f.sha256,
    read_ranges=[[0,23071]],artifacts=refs)
for role in ['worker','master']:
    receipt=dict(base,role=role,reviewer='controller' if role=='master' else 'worker A')
    if role=='master': receipt['manual_review']=dict(decision='accepted',reviewer='controller',evidence=rec(Q/'review.md'),findings='Current626-line source,685-line frozen baseline,153-line report and180-line tests independently read; runtime production prefix and bounded real entry/owner consumers reviewed.39 fresh structural checks pass,0 product runs. Only080 file understanding accepted; install/recovery boundaries and historical startup failures preserved.')
    g.atomic(R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json',json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
    text,count=re.subn(r'^- \[[ _x]\]( \*\*ZS1-080\*\*)','- '+mark+r'\1',(R/v.BLUEPRINT).read_text(),flags=re.M)
    assert count==1
    updated=v.parse(text)
    assert updated.requirement==bp.requirement
    g.atomic(R/v.BLUEPRINT,text)
    active=read(R/v.SELECTOR);active['snapshot_sha256']=updated.snapshot
    g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():g.atomic(R/rel,v.tsv(fields,rows).decode())
    front=v.frontiers(updated,read(R/v.EVIDENCE/'claims.json')['claims'],sel['run_id'],3)
    for p in (R/v.EVIDENCE).glob('todos_*.md'):g.atomic(p,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))
state('[_]')
pre=v.validate(R,item=KEY);dump(Q/'before-promotion.json',pre);assert pre['ok'],pre
state('[x]')
post=v.validate(R,item=KEY);dump(Q/'after-promotion.json',post);assert post['ok'],post
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY]
start=datetime.datetime.now(datetime.timezone.utc).isoformat()
result=subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(result.stdout);(Q/'gstage.stderr.log').write_bytes(result.stderr)
dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=result.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')))
assert result.returncode==0,result.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main()
print(json.dumps(dict(accepted=KEY,counts=post['counts'],snapshot=post['snapshot_sha256'])))

"""Single-use directory052 publication; never rerun after success."""
from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile
ROOT=Path('/Users/wangweiyang/GitHub/zenpi');D=Path(__file__).resolve().parent
W=D/'worker';KEY='ZS1-052';OUT=ROOT/'Docs/quality/stage1/ZS1-052/master-review-3.1.20'
sys.path.insert(0,str(ROOT/'tools'))
import validate_stage1_blueprint as v
from generate_stage1_gantt import atomic
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
bp=v.parse((ROOT/v.BLUEPRINT).read_text());sel=v.selector(ROOT,bp,v.BLUEPRINT)
scope,folder,report=bp.folders[KEY]
assert (scope,folder)==('source','packages/coding-agent/src/core/extensions')
assert bp.items[KEY].state=='[ ]' and list(bp.items[KEY].depends)==['ZS1-022','ZS1-023']
assert not OUT.exists() and not (ROOT/report).exists()
assert sha(W/'manifest.json')=='653a26e30b8cf0d3a6967ed61dae52da849f8c61c88e4668d476ec298fc4be4d'
assert sha(D/'final-report.md')=='ca04f3af1231003e56053178d5c38dc4a0e79a1f0b6056fef1faebc9b256581b'
assert (D/'final-report.md').read_bytes()==(W/'report.md').read_bytes()+(D/'append.md').read_bytes()
assert json.loads((D/'semantic-current.json').read_text())['passed']
offline=json.loads((D/'offline-verification.json').read_text())
assert offline['exit_code']==0 and json.loads(offline['stdout'])['ok']
for role in ['worker','master']:assert not (ROOT/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json').exists()
source=Path(bp.header['source_repo'])
sizes=v.file_hashes(bp,ROOT,source,target=False)
for key in bp.items[KEY].depends:
    for role in ['worker','master']:v.check_receipt(ROOT,bp,key,v.EVIDENCE,sel,sizes,role)
def continuity():
    directory=json.loads((D/'current-directory.json').read_text())
    parent=Path(directory['path'])
    assert sorted(p.name for p in parent.iterdir())==[a['name'] for a in directory['entries']]
    for a in directory['entries']:
        p=parent/a['name'];assert p.is_file() and not p.is_symlink() and sha(p)==a['sha256']
    for a in json.loads((D/'current-context.json').read_text()):assert sha(Path(a['path']))==a['file_sha256']
    for a in json.loads((D/'child-continuity.json').read_text()):
        assert sha(ROOT/a['receipt'])==a['receipt_sha256']
        assert sha(ROOT/a['canonical'])==a['canonical_sha256']
continuity()
backup=D/'authority-before';backup.mkdir()
for rel in [v.BLUEPRINT,v.SELECTOR,str(Path(v.EVIDENCE)/'claims.json'),*v.scaffold(bp,sizes,v.EVIDENCE)]:
    q=backup/rel;q.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/rel,q)
runs=[]
for argv in [['git','apply','--check',str(W/'create-report.patch')],['git','apply',str(W/'create-report.patch')]]:
    r=subprocess.run(argv,cwd=ROOT,capture_output=True,text=True)
    runs.append(dict(argv=argv,exit_code=r.returncode,stdout=r.stdout,stderr=r.stderr))
    (D/'apply-results.json').write_text(json.dumps(runs,indent=2)+'\n')
    assert r.returncode==0,r.stderr
assert (ROOT/report).read_bytes()==(W/'report.md').read_bytes()
with (ROOT/report).open('ab') as f:f.write((D/'append.md').read_bytes())
assert (ROOT/report).read_bytes()==(D/'final-report.md').read_bytes()
OUT.mkdir(parents=True)
for name in ['review.md','append.md','final-report.md','current-context.json','current-directory.json','child-continuity.json','semantic-current.json','offline-verification.json','prepare.py','prepare-attempt1.json','apply-results.json','accept.py']:
    shutil.copy2(D/name,OUT/name)
shutil.copy2(W/'manifest.json',OUT/'worker-manifest.json')
with tarfile.open(OUT/'worker-ready.tar.gz','w:gz') as t:t.add(W,arcname='worker')
def artifact(p):return dict(path=str(p.relative_to(ROOT)),bytes=p.stat().st_size,sha256=sha(p))
records=[artifact(ROOT/report)]+[artifact(p) for p in sorted(OUT.iterdir()) if p.is_file()]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='directory052-current-children-3.1.20',integrated_revision=v.repository_head(ROOT),validators=bp.items[KEY].validators,scope=scope,folder_path=folder,children=list(bp.items[KEY].depends),artifacts=records)
for role in ['worker','master']:
    record={**base,'role':role,'reviewer':'controller' if role=='master' else 'worker-A directory candidate independently reconciled by controller'}
    if role=='master':record['manual_review']=dict(decision='accepted',reviewer='controller',findings='Current two direct child acceptances and unchanged full-source read lineage; five physical files and zero directories; all23 current connections reconciled,17 context fragments newly read, historical nine-helper fixture read without rerun. Registration, binding, dispatch, error/cancel/reload differences independently reviewed. Frozen directory052 understanding only.',evidence=artifact(OUT/'review.md'))
    atomic(ROOT/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json',json.dumps(record,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
    text,count=re.subn(r'^- \[[ _x]\]( \*\*ZS1-052\*\*)','- '+mark+r'\1',(ROOT/v.BLUEPRINT).read_text(),flags=re.M);assert count==1
    current=v.parse(text);assert current.requirement==bp.requirement
    atomic(ROOT/v.BLUEPRINT,text)
    active=json.loads((ROOT/v.SELECTOR).read_text());active['snapshot_sha256']=current.snapshot
    atomic(ROOT/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(current,sizes,v.EVIDENCE).items():atomic(ROOT/rel,v.tsv(fields,rows).decode())
    claims=json.loads((ROOT/v.EVIDENCE/'claims.json').read_text())['claims']
    frontier=v.frontiers(current,claims,sel['run_id'],3)
    atomic(ROOT/v.EVIDENCE/f'todos_{datetime.date.today():%Y%m%d}.md',v.todo(current,frontier,v.BLUEPRINT,str(Path(v.EVIDENCE)/'claims.json')))
    return current
state('[_]');pre=v.validate(ROOT,item=KEY)
(OUT/'before-promotion.json').write_text(json.dumps(pre,ensure_ascii=False,indent=2)+'\n');assert pre['ok'],pre['errors']
final=state('[x]');post=v.validate(ROOT,item=KEY)
(OUT/'after-promotion.json').write_text(json.dumps(post,ensure_ascii=False,indent=2)+'\n');assert post['ok'],post['errors']
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY]
start=datetime.datetime.now(datetime.timezone.utc).isoformat()
r=subprocess.run(argv,cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(OUT/'gstage-final.log').write_bytes(r.stdout)
(OUT/'gstage-final.run.json').write_text(json.dumps(dict(argv=argv,cwd=str(ROOT),started_at=start,exit_code=r.returncode,log_sha256=hashlib.sha256(r.stdout).hexdigest()),indent=2)+'\n')
assert r.returncode==0 and r.stdout.startswith(b'PASS: ') and json.loads(r.stdout[6:])['ok']
continuity()
files={p.name:dict(bytes=p.stat().st_size,sha256=sha(p)) for p in sorted(OUT.iterdir()) if p.is_file()}
(OUT/'manifest.json').write_text(json.dumps(dict(item=KEY,scope='frozen directory understanding',master_accepted=True,files=files),indent=2)+'\n')
print(json.dumps(dict(accepted=KEY,counts={s:sum(i.state==s for i in final.items.values()) for s in ['[x]','[_]','[ ]']},snapshot=final.snapshot,publication_artifacts=len(files),manifest_sha256=sha(OUT/'manifest.json'),product_unchanged=True)))

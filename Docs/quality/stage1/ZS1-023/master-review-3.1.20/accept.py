"""Single-use source023 publication after independent semantic review; never rerun."""
from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile
ROOT=Path('/Users/wangweiyang/GitHub/zenpi'); D=Path(__file__).resolve().parent; W=D/'worker'; KEY='ZS1-023'
OUT=ROOT/'Docs/quality/stage1/ZS1-023/master-review-3.1.20'
sys.path.insert(0,str(ROOT/'tools'))
import validate_stage1_blueprint as v
from generate_stage1_gantt import atomic
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
bp=v.parse((ROOT/v.BLUEPRINT).read_text());sel=v.selector(ROOT,bp,v.BLUEPRINT);file=bp.files[KEY]
assert bp.items[KEY].state=='[ ]' and not OUT.exists() and not (ROOT/file.artifact).exists()
assert all(bp.items[x].state=='[x]' for x in bp.items[KEY].depends)
assert sha(W/'manifest.json')=='3f0aadf8cc8f0d6908dfef2a0eb909f7da3228415b5620ea494fb10cb7f497e3'
manifest=json.loads((W/'manifest.json').read_text())
for name,a in manifest['artifacts'].items():
    p=W/name;assert sha(p)==a['sha256'] and p.stat().st_size==a['bytes']
source=Path(bp.header['source_repo'])/file.path
assert source.read_bytes()==(W/'evidence/source/runner.ts').read_bytes()
for a in json.loads((D/'current-context.json').read_text()):assert sha(Path(a['path']))==a['sha256']
assert sha(D/'final-report.md')=='2b80ebfd84522d809b8cc4c64fc6895e97c8024c63f5686dcf2084f87f58b185'
assert (D/'final-report.md').read_bytes()==(W/manifest['report_path']).read_bytes()+(D/'append.md').read_bytes()
run=json.loads((D/'offline-verification.run.json').read_text());assert run['exit_code']==0 and run['log_sha256']==sha(D/'offline-verification.log')
for role in ['worker','master']:assert not (ROOT/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json').exists()
sizes=v.file_hashes(bp,ROOT,Path(bp.header['source_repo']),target=False)
backup=D/'authority-before';backup.mkdir()
for rel in [v.BLUEPRINT,v.SELECTOR,str(Path(v.EVIDENCE)/'claims.json'),*v.scaffold(bp,sizes,v.EVIDENCE)]:
    p=backup/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/rel,p)
ops=[]
for flags in [['--check'],[]]:
    argv=['git','apply',*flags,str(W/'candidate.patch')];start=datetime.datetime.now(datetime.timezone.utc).isoformat()
    p=subprocess.run(argv,cwd=ROOT,capture_output=True)
    ops.append(dict(argv=argv,cwd=str(ROOT),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,stdout=p.stdout.decode(),stderr=p.stderr.decode()))
    (D/'apply.json').write_text(json.dumps(ops,indent=2)+'\n');assert p.returncode==0,ops[-1]
assert (ROOT/file.artifact).read_bytes()==(W/manifest['report_path']).read_bytes()
atomic(ROOT/file.artifact,(D/'final-report.md').read_text())
OUT.mkdir(parents=True)
for p in D.iterdir():
    if p.is_file():shutil.copy2(p,OUT/p.name)
shutil.copy2(W/'manifest.json',OUT/'worker-manifest.json')
with tarfile.open(OUT/'worker-ready.tar.gz','x:gz') as t:
    for p in sorted(W.rglob('*')):
        if p.is_file():t.add(p,arcname=str(p.relative_to(W)),recursive=False)
def artifact(p):return dict(path=str(p.relative_to(ROOT)),bytes=p.stat().st_size,sha256=sha(p))
records=[artifact(ROOT/file.artifact)]+[artifact(p) for p in sorted(OUT.iterdir()) if p.is_file() and p.read_bytes().strip()]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='source023-current-3.1.20',integrated_revision=v.repository_head(ROOT),validators=bp.items[KEY].validators,source_path=file.path,source_hash=file.sha256,read_ranges=json.loads((D/'source-read.json').read_text())['read_ranges'],artifacts=records)
for role in ['worker','master']:
    receipt={**base,'role':role,'reviewer':'controller' if role=='master' else 'worker-B source candidate with controller qualification'}
    if role=='master':receipt['manual_review']=dict(decision='accepted',reviewer='controller',findings='Complete1286-line source and193-line candidate independently read; all event families, registry policies, UI prompt, provider binding, stale context, error and mutation rules covered;19 exact current bounded references inspected; read-only test assertions distinguished from runtime; uncaught unsubscribe clarification appended. Source023 only, not115 or052.',evidence=artifact(OUT/'review.md'))
    atomic(ROOT/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json',json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
    text,count=re.subn(r'^- \[[ _x]\]( \*\*ZS1-023\*\*)','- '+mark+r'\1',(ROOT/v.BLUEPRINT).read_text(),flags=re.M);assert count==1
    current=v.parse(text);assert current.requirement==bp.requirement
    atomic(ROOT/v.BLUEPRINT,text);active=json.loads((ROOT/v.SELECTOR).read_text());active['snapshot_sha256']=current.snapshot
    atomic(ROOT/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(current,sizes,v.EVIDENCE).items():atomic(ROOT/rel,v.tsv(fields,rows).decode())
    claims=json.loads((ROOT/v.EVIDENCE/'claims.json').read_text())['claims'];frontier=v.frontiers(current,claims,sel['run_id'],3)
    atomic(ROOT/v.EVIDENCE/f'todos_{datetime.date.today():%Y%m%d}.md',v.todo(current,frontier,v.BLUEPRINT,str(Path(v.EVIDENCE)/'claims.json')))
    return current
state('[_]');pre=v.validate(ROOT,item=KEY);(OUT/'before-promotion.json').write_text(json.dumps(pre,ensure_ascii=False,indent=2)+'\n');assert pre['ok'],pre['errors']
final=state('[x]');post=v.validate(ROOT,item=KEY);(OUT/'after-promotion.json').write_text(json.dumps(post,ensure_ascii=False,indent=2)+'\n');assert post['ok'],post['errors']
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY];start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(argv,cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(OUT/'gstage-final.log').write_bytes(p.stdout);(OUT/'gstage-final.run.json').write_text(json.dumps(dict(argv=argv,cwd=str(ROOT),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,log_sha256=hashlib.sha256(p.stdout).hexdigest()),indent=2)+'\n')
assert p.returncode==0 and p.stdout.startswith(b'PASS: '),p.stdout
assert json.loads(p.stdout[len(b'PASS: '):])['ok']
for a in json.loads((D/'current-context.json').read_text()):assert sha(Path(a['path']))==a['sha256']
assert sha(source)==file.sha256
files={p.name:dict(bytes=p.stat().st_size,sha256=sha(p)) for p in sorted(OUT.iterdir()) if p.is_file()}
(OUT/'manifest.json').write_text(json.dumps(dict(item=KEY,scope='frozen source understanding',master_accepted=True,files=files),indent=2)+'\n')
print(json.dumps(dict(accepted=KEY,counts={s:sum(i.state==s for i in final.items.values()) for s in ('[x]','[_]','[ ]')},snapshot=final.snapshot,publication_artifacts=len(files),manifest_sha256=sha(OUT/'manifest.json'))))

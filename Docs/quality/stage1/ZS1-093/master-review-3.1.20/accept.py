"""One-time per-directory acceptance; does not modify product files."""
from pathlib import Path
import datetime,hashlib,json,re,shutil,subprocess,sys,tarfile
ROOT=Path('/Users/wangweiyang/GitHub/zenpi');D=Path(__file__).resolve().parent
KEY='ZS1-093';W=D/'worker';OUT=ROOT/'Docs/quality/stage1/ZS1-093/master-review-3.1.20'
sys.path.insert(0,str(ROOT/'tools'))
import validate_stage1_blueprint as v
from generate_stage1_gantt import atomic
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
bp=v.parse((ROOT/v.BLUEPRINT).read_text());sel=v.selector(ROOT,bp,v.BLUEPRINT)
scope,folder,report=bp.folders[KEY]
assert bp.items[KEY].state=='[ ]' and not OUT.exists()
assert (scope,folder)==('target','.github') and list(bp.items[KEY].depends)==['ZS1-092']
assert all(bp.items[c].state=='[x]' for c in bp.items[KEY].depends)
assert sha(W/'manifest.json')=='3f0fef477f78ff06555613ca852c50bf35dc0e1af89aacbc07230b1d26d7e84e'
m=json.loads((W/'manifest.json').read_text());payload=[a['path'] for a in m['files']]
assert len(payload)==len(set(payload))==114 and report in payload
for a in m['files']:
 p=W/'files'/a['path'];assert p.stat().st_size==a['bytes'] and sha(p)==a['sha256']
 assert a['path'].startswith('Docs/') and '..' not in Path(a['path']).parts and not (ROOT/a['path']).exists()
for role in ['worker','master']:assert not (ROOT/v.EVIDENCE/'receipts'/(KEY+'.'+role+'.json')).exists()
assert json.loads((D/'semantic-current.json').read_text())['passed']
assert sha(D/'final-report.md')=='c853a5988d8ff4a35e83d789175433c173bd05a0c08cc82bd0bba39f7ee1e589'
assert (D/'final-report.md').read_bytes()==(W/'files'/report).read_bytes()+(D/'append.md').read_bytes()
run=json.loads((D/'offline-verification.run.json').read_text());assert run['exit_code']==0 and run['log_sha256']==sha(D/'offline-verification.log')
inventory=json.loads((D/'current-directory.json').read_text())['entries']
assert sorted(p.name for p in (ROOT/folder).iterdir())==[a['name'] for a in inventory]
for a in inventory:assert (ROOT/folder/a['name']).is_dir() and not (ROOT/folder/a['name']).is_symlink()
for a in json.loads((D/'descendant-continuity.json').read_text()):assert sha(ROOT/a['path'])==a['sha256']
sizes=v.file_hashes(bp,ROOT,Path(bp.header['source_repo']),target=False)
for child in bp.items[KEY].depends:
 for role in ['worker','master']:
  v.check_receipt(ROOT,bp,child,v.EVIDENCE,sel,sizes,role)
  assert (D/f'child092.{role}.json').read_bytes()==(ROOT/v.EVIDENCE/'receipts'/f'{child}.{role}.json').read_bytes()
backup=D/'authority-before';backup.mkdir()
for rel in [v.BLUEPRINT,v.SELECTOR,str(Path(v.EVIDENCE)/'claims.json'),*v.scaffold(bp,sizes,v.EVIDENCE)]:
 q=backup/rel;q.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/rel,q)
for rel in payload:
 q=ROOT/rel;q.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(D/'final-report.md' if rel==report else W/'files'/rel,q)
OUT.mkdir()
for name in ['review.md','append.md','final-report.md','final-report.patch','current-directory.json','semantic-current.json','descendant-continuity.json','child092.master.json','child092.worker.json','child092-report.md','offline-verification.log','offline-verification.run.json','accept.py']:
 shutil.copy2(D/name,OUT/name)
shutil.copy2(W/'manifest.json',OUT/'worker-manifest.json')
with tarfile.open(OUT/'worker-ready.tar.gz','w:gz') as t:
 for p in sorted(W.rglob('*')):
  if p.is_file():t.add(p,arcname=str(p.relative_to(W)),recursive=False)
def artifact(p):return dict(path=str(p.relative_to(ROOT)),bytes=p.stat().st_size,sha256=sha(p))
all_paths=[ROOT/rel for rel in payload]+[p for p in OUT.iterdir() if p.is_file()]
empty=[artifact(p) for p in all_paths if not p.read_bytes().strip()]
(OUT/'empty-historical-artifacts.json').write_text(json.dumps({'reason':'Preserved historical zero-byte files are indexed, not standalone proof','files':empty},indent=2)+'\n')
records=[artifact(p) for p in all_paths if p.read_bytes().strip()]+[artifact(OUT/'empty-historical-artifacts.json')]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,
 baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='directory093-current-child-3.1.20',
 integrated_revision=v.repository_head(ROOT),validators=bp.items[KEY].validators,scope=scope,folder_path=folder,
 children=list(bp.items[KEY].depends),artifacts=records)
for role in ['worker','master']:
 record={**base,'role':role,'reviewer':'controller' if role=='master' else 'worker-C provisional directory report with controller current-child reconciliation'}
 if role=='master':record['manual_review']=dict(decision='accepted',reviewer='controller',findings='Full parent report and current child092 report read; physical zero files and one directory, exact direct child092 acceptance and transitive089 chain, unchanged context-only scope and current workflow boundaries independently reconciled. Accept frozen parent093 only.',evidence=artifact(OUT/'review.md'))
 atomic(ROOT/v.EVIDENCE/'receipts'/(KEY+'.'+role+'.json'),json.dumps(record,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
 text,count=re.subn(r'^- \[[ _x]\]( \*\*ZS1-093\*\*)','- '+mark+r'\1',(ROOT/v.BLUEPRINT).read_text(),flags=re.M);assert count==1
 current=v.parse(text);assert current.requirement==bp.requirement
 atomic(ROOT/v.BLUEPRINT,text);active=json.loads((ROOT/v.SELECTOR).read_text());active['snapshot_sha256']=current.snapshot
 atomic(ROOT/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
 for rel,(fields,rows) in v.scaffold(current,sizes,v.EVIDENCE).items():atomic(ROOT/rel,v.tsv(fields,rows).decode())
 claims=json.loads((ROOT/v.EVIDENCE/'claims.json').read_text())['claims'];frontier=v.frontiers(current,claims,sel['run_id'],3)
 atomic(ROOT/v.EVIDENCE/f'todos_{datetime.date.today():%Y%m%d}.md',v.todo(current,frontier,v.BLUEPRINT,str(Path(v.EVIDENCE)/'claims.json')))
 return current
pending=state('[_]');pre=v.validate(ROOT,item=KEY);(OUT/'before-promotion.json').write_text(json.dumps(pre,ensure_ascii=False,indent=2)+'\n');assert pre['ok'],pre['errors']
final=state('[x]');post=v.validate(ROOT,item=KEY);(OUT/'after-promotion.json').write_text(json.dumps(post,ensure_ascii=False,indent=2)+'\n');assert post['ok'],post['errors']
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY];start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(argv,cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(OUT/'gstage-final.log').write_bytes(p.stdout);(OUT/'gstage-final.run.json').write_text(json.dumps(dict(argv=argv,cwd=str(ROOT),started_at=start,exit_code=p.returncode,log_sha256=hashlib.sha256(p.stdout).hexdigest()),indent=2)+'\n')
assert p.returncode==0 and p.stdout.startswith(b'PASS: '),p.stdout
assert json.loads(p.stdout[len(b'PASS: '):])['ok']
for a in inventory:assert (ROOT/folder/a['name']).is_dir() and not (ROOT/folder/a['name']).is_symlink()
for a in json.loads((D/'descendant-continuity.json').read_text()):assert sha(ROOT/a['path'])==a['sha256']
files={p.name:{'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(OUT.iterdir()) if p.is_file()}
(OUT/'manifest.json').write_text(json.dumps(dict(item=KEY,scope='frozen directory understanding',master_accepted=True,files=files),indent=2)+'\n')
print(json.dumps(dict(accepted=KEY,counts={s:sum(i.state==s for i in final.items.values()) for s in ('[x]','[_]','[ ]')},publication_artifacts=len(files),manifest_sha256=sha(OUT/'manifest.json'),product_unchanged=True)))

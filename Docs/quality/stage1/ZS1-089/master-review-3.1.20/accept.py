from pathlib import Path
import datetime, hashlib, json, re, shutil, sys, tarfile

ROOT = Path('/Users/wangweiyang/GitHub/zenpi')
D = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT/'tools'))
import validate_stage1_blueprint as v
from generate_stage1_gantt import atomic

KEY = 'ZS1-089'
W = D/'worker'
OUT = ROOT/'Docs/quality/stage1/ZS1-089/master-review-3.1.20'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
bp = v.parse((ROOT/v.BLUEPRINT).read_text())
sel = v.selector(ROOT,bp,v.BLUEPRINT)
assert bp.items[KEY].state == '[ ]' and not OUT.exists()
assert all(bp.items[d].state == '[x]' for d in bp.items[KEY].depends)
assert sha(W/'manifest.json') == '057473cb8affc99220939927fe78d01e17fe3dc1c3f598651a19f79f1bc02f1b'
manifest = json.loads((W/'manifest.json').read_text())
for path, record in manifest['artifacts'].items():
    assert sha(W/path) == record['sha256'] and (W/path).stat().st_size == record['bytes']
for record in manifest['files']:
    path = W/'files'/record['path']
    assert sha(path) == record['sha256'] and path.stat().st_size == record['bytes']
assert sha(ROOT/'.github/workflows/ci.yml') == '44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9'
for name in ('offline-verification','semantic-current'):
    run = json.loads((D/(name+'.run.json')).read_text())
    assert run['exit_code'] == 0 and run['log_sha256'] == sha(D/(name+'.log'))
payload = [row['path'] for row in manifest['files']]
assert len(payload) == len(set(payload)) == 337
assert all(p.startswith('Docs/') and '..' not in Path(p).parts for p in payload)
assert all(not (ROOT/p).exists() for p in payload)
assert bp.files[KEY].sha256 == '5d7173fa20a0de949e53525018dad07718a9e4a9d24a5fcd137b056af31792d6'
assert all(not (ROOT/v.EVIDENCE/'receipts'/(KEY+'.'+role+'.json')).exists() for role in ('worker','master'))
assert sha(W/'files'/bp.files[KEY].artifact) == 'cad5882b88c6014a5d9d9f755e63cdcbf4a89b424801cbc0d52cc86fabcf0891'
sizes = v.file_hashes(bp,ROOT,Path(bp.header['source_repo']),target=False)
backup = D/'authority-before'
backup.mkdir()
surfaces = [v.BLUEPRINT,v.SELECTOR,str(Path(v.EVIDENCE)/'claims.json')]
surfaces += list(v.scaffold(bp,sizes,v.EVIDENCE))
for rel in surfaces:
    dest = backup/rel
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(ROOT/rel,dest)
for rel in payload:
    dest = ROOT/rel
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(W/'files'/rel,dest)
OUT.mkdir()
for name in ('review.md','verify_current.py','offline-verification.log','offline-verification.run.json',
             'semantic-current.log','semantic-current.run.json','accept.py'):
    shutil.copy2(D/name,OUT/name)
shutil.copy2(W/'manifest.json',OUT/'worker-manifest.json')
with tarfile.open(OUT/'worker-ready.tar.gz','w:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file(): archive.add(p,arcname=str(p.relative_to(W)),recursive=False)
coverage = dict(source='.github/workflows/ci.yml',baseline_sha256=bp.files[KEY].sha256,
    baseline_bytes=2480,baseline_lines=77,baseline_read_ranges=[[0,2480]],
    current_sha256=sha(ROOT/'.github/workflows/ci.yml'),current_bytes=3973,current_lines=113,current_read_ranges=[[0,3973]],
    report_sha256=sha(ROOT/bp.files[KEY].artifact),report_read_lines=[[1,70],[71,148],[149,227]],
    source_functions=0,source_tests=0,context_only=False,
    previous_bytes=3104,previous_lines=94,previous_read_ranges=[[0,3104]],
    acceptance_scope='complete CI file understanding; no hosted workflow or sibling owner acceptance')
(OUT/'read-coverage.json').write_text(json.dumps(coverage,indent=2)+'\n')
def artifact(rel):
    p = ROOT/rel
    return dict(path=rel,bytes=p.stat().st_size,sha256=sha(p))
all_paths = payload + [str(p.relative_to(ROOT)) for p in OUT.iterdir() if p.is_file()]
receipt = dict(schema_version='stage1-receipt/v1',item_id=KEY,role='master',run_id=sel['run_id'],
    requirement_digest=bp.requirement,baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],
    complete=True,reviewer='controller',attempt_id='master-ci089-3.1.20',integrated_revision=v.repository_head(ROOT),
    validators=bp.items[KEY].validators,source_path=bp.files[KEY].path,source_hash=bp.files[KEY].sha256,
    read_ranges=[[0,2480]],current_source_hash=sha(ROOT/'.github/workflows/ci.yml'),current_read_ranges=[[0,3973]],
    artifacts=[artifact(p) for p in all_paths],
    manual_review=dict(decision='accepted',reviewer='controller',
        findings='Complete 77/94/113-line CI versions and 227-line report independently read; YAML semantics, exact two-step delta, current contexts, twelve Bash syntax checks and full offline evidence verified; historical five shell scenarios not recounted as new; hosted Actions and product gates remain unproven.',
        evidence=artifact(str((OUT/'review.md').relative_to(ROOT)))))
worker = {k:value for k,value in receipt.items() if k != 'manual_review'}
worker.update(role='worker',reviewer='worker-C',attempt_id='worker-ci089-review-3.1.20',artifacts=[artifact(p) for p in payload])
receipts = ROOT/v.EVIDENCE/'receipts'
for role,record in (('worker',worker),('master',receipt)):
    assert not (receipts/(KEY+'.'+role+'.json')).exists()
    atomic(receipts/(KEY+'.'+role+'.json'),json.dumps(record,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
    original = (ROOT/v.BLUEPRINT).read_text()
    text,count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-089\*\*)','- '+mark+r'\1',original,flags=re.M)
    assert count == 1
    changed = v.parse(text)
    assert changed.requirement == bp.requirement
    atomic(ROOT/v.BLUEPRINT,text)
    active = json.loads((ROOT/v.SELECTOR).read_text())
    active['snapshot_sha256'] = changed.snapshot
    atomic(ROOT/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(changed,sizes,v.EVIDENCE).items():
        atomic(ROOT/rel,v.tsv(fields,rows).decode())
    claims = json.loads((ROOT/v.EVIDENCE/'claims.json').read_text())['claims']
    frontier = v.frontiers(changed,claims,sel['run_id'],3)
    atomic(ROOT/v.EVIDENCE/f'todos_{datetime.date.today():%Y%m%d}.md',v.todo(changed,frontier,v.BLUEPRINT,str(Path(v.EVIDENCE)/'claims.json')))
    return changed
pending = state('[_]')
v.check_receipt(ROOT,pending,KEY,v.EVIDENCE,v.selector(ROOT,pending,v.BLUEPRINT),sizes,'worker')
pre = v.validate(ROOT,item=KEY)
(D/'before-promotion.json').write_text(json.dumps(pre,ensure_ascii=False,indent=2)+'\n')
assert pre['ok'],pre['errors']
final = state('[x]')
result = v.validate(ROOT,item=KEY)
(D/'after-promotion.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
assert result['ok'],result['errors']
assert sha(ROOT/'.github/workflows/ci.yml') == coverage['current_sha256']
print(json.dumps(dict(accepted=KEY,counts={s:sum(i.state==s for i in final.items.values()) for s in ('[x]','[_]','[ ]')},source_unchanged=True),ensure_ascii=False))

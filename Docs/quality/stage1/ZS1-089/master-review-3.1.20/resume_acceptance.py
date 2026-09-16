"""One-time receipt packaging correction after the preserved accept.py failure."""
from pathlib import Path
import datetime, hashlib, json, re, shutil, sys
ROOT=Path('/Users/wangweiyang/GitHub/zenpi')
D=Path(__file__).resolve().parent
OUT=ROOT/'Docs/quality/stage1/ZS1-089/master-review-3.1.20'
sys.path.insert(0,str(ROOT/'tools'))
import validate_stage1_blueprint as v
from generate_stage1_gantt import atomic
KEY='ZS1-089'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
bp=v.parse((ROOT/v.BLUEPRINT).read_text()); sel=v.selector(ROOT,bp,v.BLUEPRINT)
assert bp.items[KEY].state=='[_]' and not (D/'before-promotion.json').exists()
assert json.loads((D/'accept.run.json').read_text())['exit_code']==1
assert sha(ROOT/'.github/workflows/ci.yml')=='44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9'
m=json.loads((D/'worker/manifest.json').read_text())
for a in m['files']:
    p=ROOT/a['path']; assert p.stat().st_size==a['bytes'] and sha(p)==a['sha256']
def artifact(p): return dict(path=str(p.relative_to(ROOT)),bytes=p.stat().st_size,sha256=sha(p))
records={role:json.loads((ROOT/v.EVIDENCE/'receipts'/(KEY+'.'+role+'.json')).read_text()) for role in ('worker','master')}
empty=[a for a in records['worker']['artifacts'] if not (ROOT/a['path']).read_bytes().strip()]
assert empty and all('/reused/' in a['path'] and a['bytes']==0 for a in empty)
assert all(a['path']!=bp.files[KEY].artifact for a in empty)
index=OUT/'empty-historical-artifacts.json'
assert not index.exists()
index.write_text(json.dumps(dict(reason='Historical patch base placeholders and empty syntax output; preserved exactly, not standalone nonempty proof.',files=empty),indent=2)+'\n')
note=OUT/'receipt-recovery.md'
note.write_text('''# Receipt packaging correction

The first actual acceptance attempt exited 1 after reaching worker-self-tested state: the receipt referenced empty historical files directly, while the checker requires nonempty top-level artifacts. The failure and original receipts are preserved here. No source, report, historical payload, validator rule or test result was changed.

The corrected receipts retain all nonempty evidence and reference the complete worker archive, its manifest and an explicit index of the preserved empty files. Empty patch-base placeholders and empty syntax-output files remain in place with their original sizes and hashes; none is promoted to standalone semantic proof. The unique full report remains a direct nonempty artifact. This is a receipt packaging correction, not a new behavioral pass. Only ZS1-089 can be promoted after its actual gate succeeds.
''')
for name in ('accept.log','accept.run.json','resume_acceptance.py'):
    assert not (OUT/name).exists(); shutil.copy2(D/name,OUT/name)
for role,record in records.items():
    p=ROOT/v.EVIDENCE/'receipts'/(KEY+'.'+role+'.json')
    original=OUT/('initial-'+role+'-receipt.json'); assert not original.exists(); shutil.copy2(p,original)
    kept=[a for a in record['artifacts'] if (ROOT/a['path']).read_bytes().strip()]
    paths={a['path'] for a in kept}
    for extra in [index,note,OUT/'worker-ready.tar.gz',OUT/'worker-manifest.json',OUT/'accept.log',OUT/'accept.run.json']:
        a=artifact(extra)
        if a['path'] not in paths: kept.append(a); paths.add(a['path'])
    record['artifacts']=kept
    record['attempt_id'] += '-receipt-packaging-corrected'
    atomic(p,json.dumps(record,ensure_ascii=False,separators=(',',':'))+'\n')
sizes=v.file_hashes(bp,ROOT,Path(bp.header['source_repo']),target=False)
v.check_receipt(ROOT,bp,KEY,v.EVIDENCE,sel,sizes,'worker')
pre=v.validate(ROOT,item=KEY)
(D/'before-promotion.json').write_text(json.dumps(pre,ensure_ascii=False,indent=2)+'\n')
assert pre['ok'],pre['errors']
text,count=re.subn(r'^- \[_\]( \*\*ZS1-089\*\*)',r'- [x]\1',bp.text,flags=re.M)
assert count==1
final=v.parse(text); assert final.requirement==bp.requirement
atomic(ROOT/v.BLUEPRINT,text)
active=json.loads((ROOT/v.SELECTOR).read_text()); active['snapshot_sha256']=final.snapshot
atomic(ROOT/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
for rel,(fields,rows) in v.scaffold(final,sizes,v.EVIDENCE).items(): atomic(ROOT/rel,v.tsv(fields,rows).decode())
claims=json.loads((ROOT/v.EVIDENCE/'claims.json').read_text())['claims']
frontier=v.frontiers(final,claims,sel['run_id'],3)
atomic(ROOT/v.EVIDENCE/f'todos_{datetime.date.today():%Y%m%d}.md',v.todo(final,frontier,v.BLUEPRINT,str(Path(v.EVIDENCE)/'claims.json')))
result=v.validate(ROOT,item=KEY)
(D/'after-promotion.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
assert result['ok'],result['errors']
assert sha(ROOT/'.github/workflows/ci.yml')=='44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9'
print(json.dumps(dict(accepted=KEY,empty_historical_files_preserved=len(empty),counts={s:sum(i.state==s for i in final.items.values()) for s in ('[x]','[_]','[ ]')},source_unchanged=True)))

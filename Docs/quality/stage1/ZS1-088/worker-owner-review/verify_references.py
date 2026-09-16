from pathlib import Path
import hashlib,json,re
R=Path(__file__).resolve().parents[5];E=Path(__file__).resolve().parent;sha=lambda b:hashlib.sha256(b).hexdigest();m=json.loads((E/'input-manifest.json').read_text());refs=json.loads((E/'references.json').read_text())
for r in refs:
 p=Path(r['path']);p=p if p.is_absolute() else R/p;b=p.read_bytes();assert sha(b)==r['sha256'] and len(b)==r['bytes']
for k in ['original_frozen','current']:
 r=m[k];b=(R/r['path']).read_bytes();assert len(b)==r['bytes'] and len(b.splitlines())==r['lines'] and sha(b)==r['sha256']
current=(R/m['current']['path']).read_bytes();assert current==Path(m['main_source_at_capture']).read_bytes();c=m['ordered_read_chunks'][0];assert c['byte_start']==0 and c['byte_end']==len(current) and c['sha256']==sha(current)
old=(R/m['original_frozen']['path']).read_text();new=current.decode();names=re.findall(r'^pub mod (\w+);$',new,re.M);oldnames=re.findall(r'^pub mod (\w+);$',old,re.M);assert len(names)==len(set(names))==40 and len(oldnames)==29 and [n for n in names if n in oldnames]==oldnames
rows=json.loads((E/'module-resolution.json').read_text());assert [r['module'] for r in rows]==names and sum(r['new_since_blueprint'] for r in rows)==11
for r in rows:
 p=Path(r['resolved_source']);candidates=[p.parent/(r['module']+'.rs'),p.parent/r['module']/'mod.rs'] if p.name!='mod.rs' else [p.parent.parent/(r['module']+'.rs'),p.parent/'mod.rs'];assert sum(p.is_file() for p in candidates)==1 and p.is_file();assert new.splitlines()[r['line']-1]==f'pub mod {r["module"]};'
context=json.loads((R/'.ops/target079-owner-ready/files/Docs/quality/stage1/ZS1-079/worker-owner-review/compile-context.json').read_text());entry=next(r for r in context['files'] if r['path']=='src/lib.rs');assert entry['sha256']==sha(current)
report=(R/m['report']).read_text();assert all(r['module'] in report for r in rows)
for k in ['original_frozen','current']:assert m[k]['sha256'] in report
print(json.dumps({'status':'pass','source_bytes':len(current),'source_lines':42,'baseline_lines':31,'public_modules':40,'retained':29,'added':11,'references':len(refs),'module_lookup':'40uniquepaths; siblingsnotsemanticallyreviewed','new_behavior_cases':0,'meaning':'index/range/hash/referenceintegrity only'},indent=2))

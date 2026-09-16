from pathlib import Path
import dataclasses,hashlib,json,re,sys
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'tools'))
import validate_stage1_blueprint as v
w=Path(__file__).resolve().parent;p=w/'frozen-input';m=json.loads((p/'manifest.json').read_text());base=p/'files';ev=base/m['evidence_directory']
sha=lambda b:hashlib.sha256(b).hexdigest()
assert sha((p/'manifest.json').read_bytes())=='de67dbe0c4c00c0889b8d0e1fd4c4f03d929df6830e91ca7b6b6f9b938a8749a'
for x in m['files']:
 b=(base/x['path']).read_bytes();assert len(b)==x['bytes'] and sha(b)==x['sha256']
 assert sha((p/x['patch_file']).read_bytes())==x['patch_sha256']
bp=v.parse((ROOT/v.BLUEPRINT).read_text());old=v.parse((ROOT/'.ops/stage1_execution/requirement-3.1.19-archive/blueprint.md').read_text())
assert dataclasses.asdict(bp.items['ZS1-015'])==dataclasses.asdict(old.items['ZS1-015']) and bp.files['ZS1-015']==old.files['ZS1-015']
f=bp.files['ZS1-015'];b=(Path(bp.header['source_repo'])/f.path).read_bytes();assert len(b)==37411 and len(b.splitlines())==881 and sha(b)==f.sha256
assert (ev/'types.ts').read_bytes()==b
input_manifest=json.loads((ev/'input-manifest.json').read_text());ranges=[];joined=b''
for chunk in input_manifest['ordered_complete_read_chunks']:
 data=(base/chunk['artifact']).read_bytes();assert data==b[chunk['byte_start']:chunk['byte_end']] and sha(data)==chunk['sha256'];joined+=data;ranges.append([chunk['byte_start'],chunk['byte_end']])
assert joined==b;v.check_ranges(ranges,len(b))
inventory=[dict(line=i,kind=x[1],name=x[2]) for i,line in enumerate(b.decode().splitlines(),1) if (x:=re.match(r'export (type|interface) (\w+)',line))]
assert inventory==json.loads((ev/'export-inventory.json').read_text()) and len(inventory)==72
refs=json.loads((ev/'target-references.json').read_text());count=0
for ref in refs:
 raw=(ROOT/ref['repository_path']).read_bytes();assert len(raw)==ref['bytes'] and sha(raw)==ref['sha256']
 for part in ref['excerpts']:
  data=(base/part['artifact']).read_bytes();assert data==raw[part['byte_start']:part['byte_end']] and sha(data)==part['sha256'];count+=1
report=base/f.artifact;raw=report.read_bytes();v.artifact(base,dict(path=f.artifact,bytes=len(raw),sha256=sha(raw)));assert f.path in raw.decode() and f.sha256 in raw.decode()
out=dict(ok=True,authority=bp.header['blueprint_version'],requirement_digest=bp.requirement,item='ZS1-015',payload_files=len(m['files']),source_sha256=f.sha256,source_bytes=len(b),source_lines=881,ranges=ranges,named_declarations=72,type_only_reexports=1,target_files=len(refs),target_excerpts=count,unchanged_obligation_since_worker_authority=True,scope='Static single-file semantic review and actual integrity/G-FILE primitives; no TypeScript typecheck, runtime, provider, directory or product acceptance')
print(json.dumps(out,indent=2))

from pathlib import Path
import hashlib,json,re,runpy,sys
ROOT=Path.cwd();EV=ROOT/'Docs/quality/stage1/ZS1-015/worker-types319';MAIN=Path('/Users/wangweiyang/GitHub/zenpi')
sha=lambda b:hashlib.sha256(b).hexdigest()
m=json.loads((EV/'input-manifest.json').read_text());b=Path(m['source_absolute_path']).read_bytes()
assert sha(b)==m['sha256']==m['frozen_record']['sha256'] and len(b)==m['bytes']==37411
assert (EV/'types.ts').read_bytes()==b and len(b.splitlines())==881
ranges=[];joined=b''
for row in m['ordered_complete_read_chunks']:
 part=(ROOT/row['artifact']).read_bytes();assert part==b[row['byte_start']:row['byte_end']] and sha(part)==row['sha256'];joined+=part;ranges.append([row['byte_start'],row['byte_end']])
assert joined==b
inventory=[{'line':i,'kind':x[1],'name':x[2]} for i,line in enumerate(b.decode().splitlines(),1) if (x:=re.match(r'export (type|interface) (\w+)',line))]
assert inventory==json.loads((EV/'export-inventory.json').read_text()) and len(inventory)==72
sys.path.insert(0,str(MAIN/'tools'));gate=runpy.run_path(str(MAIN/'tools/validate_stage1_blueprint.py'))
bp=gate['parse']((MAIN/'Docs/stage_1_v3_pi_mono_blueprint.md').read_text());file=bp.files['ZS1-015'];gate['check_ranges'](ranges,len(b))
assert file.path==m['source_path'] and file.sha256==m['sha256'] and bp.requirement==m['requirement_digest']
report=ROOT/file.artifact;r=report.read_bytes();record={'path':file.artifact,'bytes':len(r),'sha256':sha(r)}
gate['artifact'](ROOT,record);assert file.path in r.decode() and file.sha256 in r.decode()
refs=json.loads((EV/'target-references.json').read_text());count=0
for ref in refs:
 current=Path(ref['path']).read_bytes();assert len(current)==ref['bytes'] and sha(current)==ref['sha256'],ref['path']
 for part in ref['excerpts']:
  data=(ROOT/part['artifact']).read_bytes();assert data==current[part['byte_start']:part['byte_end']] and sha(data)==part['sha256'];count+=1
print(json.dumps({'ok':True,'item':'ZS1-015','authority':'3.1.19','source_bytes':len(b),'source_lines':881,'source_sha256':sha(b),'complete_read_ranges':ranges,'named_declarations':len(inventory),'target_files':len(refs),'target_excerpts':count,'report':record,'checks':'source frozen identity; complete ordered bytes; declaration inventory; G-FILE check_ranges/artifact/source identity; current target excerpt hashes','not_claimed':['TypeScript typecheck','runtime behavior','G-STAGE master acceptance','directory acceptance']},indent=2))

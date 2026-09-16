from pathlib import Path
import hashlib,json,re,shutil,sys,tarfile
R=Path(__file__).resolve().parents[3];D=Path(__file__).resolve().parent;W=D/'worker';Q=R/'Docs/quality/stage1/ZS1-123/master-file-completion-3.1.21'
assert not Q.exists()
def read(p):return json.loads(p.read_text())
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
expected=read(D/'after-build-after-inputs.json');assert all(meta(R/p)==m for p,m in expected.items())
assert read(D/'integration-result.json')['passed']
assert read(D/'worker-offline.json')['checks_count']==723 and read(D/'worker-offline.json')['failed']==0
assert meta(W/'manifest.json')['sha256']=='ea05fcb2d5db3bc36c3d954ea887b7c1bb66f3285cd8a84be8ddbd74abcad7cf'
for x in read(W/'manifest.json')['payload']:assert meta(W/x['path'])=={k:x[k] for k in ['bytes','sha256']}
for name,code in [('before-build',0),('before-negative',101),('worker-short-terminal-negative',101),('root-targeted',0),('after-regression',0),('owned-format',0),('after-build',0),('root-pty',0),('root-clippy',0)]:
 row=read(D/(name+'.run.json'));assert row['exit_code']==code;assert row['log']==meta(D/(name+'.log'))
counts=list(map(int,re.findall(r'test result: ok\. (\d+) passed; 0 failed',(D/'after-regression.log').read_text())));assert len(counts)==10 and sum(counts)==162
p=read(D/'root-pty-current/result.json');assert p['ok'] and len(p['checks'])==11 and all(p['checks'].values()) and p['http_requests']==[]
assert p['binary_sha256']==meta(D/'binaries/after-zenpi')['sha256']
assert all((R/n).read_bytes()==(D/'root-candidate'/n).read_bytes() for n in ['src/tui.rs','tests/tui_command_palette.rs'])
sys.path.insert(0,str(R/'tools'));import validate_stage1_blueprint as v
bp=v.parse((R/v.BLUEPRINT).read_text());assert bp.items['ZS1-123'].state=='[ ]';assert v.validate(R)['ok']
Q.mkdir(parents=True)
for p in D.iterdir():
 if p.is_file():shutil.copy2(p,Q/('review.md' if p.name=='root-review.md' else p.name))
shutil.copytree(D/'root-pty-current',Q/'root-pty-current')
for folder in ['worker','binaries','root-candidate']:
 with tarfile.open(Q/(folder+'.tar.gz'),'x:gz') as tar:
  for p in sorted((D/folder).rglob('*')):
   if p.is_file():tar.add(p,arcname=p.relative_to(D/folder).as_posix(),recursive=False)
result=dict(item='ZS1-123',complete=False,master_accepted=False,local_fix_integrated=True,regression_tests=162,additional_distinct_lib_tests=3,root_pty_checks=11,root_pty_runs=1,product_files={p:meta(R/p) for p in ['src/tui.rs','tests/tui_command_palette.rs']},blueprint_snapshot=bp.snapshot,requirement_digest=bp.requirement,scope='File completion partial/empty status and short-terminal visibility; no full123/TUI/release/budget acceptance.')
dump(D/'master-verification.json',result);dump(Q/'master-verification.json',result)
dump(Q/'manifest.json',dict(item='ZS1-123',complete=False,local_fix_integrated=True,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
assert all(meta(R/p)==m for p,m in expected.items());print(json.dumps(result,ensure_ascii=False))

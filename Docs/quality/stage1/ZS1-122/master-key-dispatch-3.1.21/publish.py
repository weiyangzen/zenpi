from pathlib import Path
import datetime,hashlib,json,re,shutil,subprocess,sys,tarfile
R=Path(__file__).resolve().parents[3]
D=Path(__file__).resolve().parent
W=D/'worker'
Q=R/'Docs/quality/stage1/ZS1-122/master-key-dispatch-3.1.21'
assert not Q.exists()
def read(p):return json.loads(p.read_text())
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
expected=read(D/'after-build-after-inputs.json')
assert all(meta(R/p)==m for p,m in expected.items())
assert read(D/'integration-result.json')['passed']
assert read(D/'worker-offline.json')['checks']==595
assert read(D/'worker-offline.json')['failed']==0
assert meta(W/'manifest.json')['sha256']=='c1ecdc7f97eb10be0448744fce31abf7a88fc9c8764b7873a3f5362c6224baba'
for p,m in read(W/'manifest.json')['files'].items():assert meta(W/p)==m
for name,code in [('before-build',0),('before-negative',101),('after-regression',0),('owned-format',0),('after-build',0),('global-format',0),('cargo-check',0),('root-pty-before',0),('root-pty-after',0)]:
 record=read(D/(name+'.run.json'));assert record['exit_code']==code;assert record['log']==meta(D/(name+'.log'))
counts=list(map(int,re.findall(r'test result: ok\. (\d+) passed; 0 failed',(D/'after-regression.log').read_text())))
assert len(counts)==10 and sum(counts)==157
for label in ['before','after']:
 result=read(D/('root-pty-'+label)/'result.json');assert result['error'] is None and result['exit_code']==0
 assert result['binary_sha256']==meta(D/'binaries'/(label+'-zenpi'))['sha256']
 assert result['checks']['menu_hint_present']==(label=='before')
 assert all(x for k,x in result['checks'].items() if k!='menu_hint_present')
 assert read(D/('root-pty-'+label)/'requests.json')==[]
 assert not (D/('root-pty-'+label)/'unexpected-editor-invocation').exists()
assert all((R/p).read_bytes()==(W/'candidate'/p).read_bytes() for p in ['src/tui.rs','tests/tui_composer.rs'])
sys.path.insert(0,str(R/'tools'))
import validate_stage1_blueprint as v
bp=v.parse((R/v.BLUEPRINT).read_text());assert bp.items['ZS1-122'].state=='[ ]'
assert v.validate(R)['ok']
Q.mkdir(parents=True)
for p in D.iterdir():
 if p.is_file() and p.name!='claims-before.json':shutil.copy2(p,Q/('review.md' if p.name=='root-review.md' else p.name))
for label in ['before','after']:shutil.copytree(D/('root-pty-'+label),Q/('root-pty-'+label))
for dirname in ['worker','binaries']:
 with tarfile.open(Q/(dirname+'.tar.gz'),'x:gz') as tar:
  for p in sorted((D/dirname).rglob('*')):
   if p.is_file():tar.add(p,arcname=p.relative_to(D/dirname).as_posix(),recursive=False)
result=dict(item='ZS1-122',complete=False,master_accepted=False,local_fix_integrated=True,regression_tests=157,root_pty_runs=2,
 product_files={p:meta(R/p) for p in ['src/tui.rs','tests/tui_composer.rs']},blueprint_snapshot=bp.snapshot,requirement_digest=bp.requirement,
 scope='Local input dispatch and footer repair; no full122, target file, release or cold-start acceptance.')
dump(D/'master-verification.json',result);dump(Q/'master-verification.json',result)
dump(Q/'manifest.json',dict(item='ZS1-122',complete=False,local_fix_integrated=True,artifacts={p.relative_to(Q).as_posix():meta(p) for p in Q.rglob('*') if p.is_file()}))
assert all(meta(R/p)==m for p,m in expected.items())
print(json.dumps(result,ensure_ascii=False))

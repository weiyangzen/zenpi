"""Offline integrity and exact single-file patch replay; no Cargo/product execution."""
from pathlib import Path
import json,hashlib,subprocess,tempfile
root=Path(__file__).resolve().parent
def sha(b):return hashlib.sha256(b).hexdigest()
def run(argv,cwd):
 p=subprocess.run(argv,cwd=cwd,capture_output=True,text=True);assert p.returncode==0,(argv,p.stdout,p.stderr);return p.stdout
m=json.loads((root/'manifest.json').read_text());assert m['item']=='ZS1-125' and m['product_paths']==['src/render.rs'] and not m['complete']
expected=set(m['artifacts']);actual={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and p!=root/'manifest.json'};assert expected==actual
for rel,r in m['artifacts'].items():
 p=Path(rel);assert not p.is_absolute() and '..' not in p.parts
 b=(root/p).read_bytes();assert len(b)==r['bytes'] and sha(b)==r['sha256'],rel
before=(root/'before/src/render.rs').read_bytes();after=(root/'after/src/render.rs').read_bytes()
assert sha(before)=='6eaaa712745659ae48f429217c5eb8961dd12e71789f8a2bc70867ee71ba25a7'
assert sha(after)=='b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d'
for p in (root/'commands').glob('*.json'):
 r=json.loads(p.read_text());b=(root/'commands'/r['log']).read_bytes();assert len(b)==r['log_bytes'] and sha(b)==r['log_sha256']
 assert r['HOME_preserved'] and r['CODEX_HOME_preserved']
for name in ['isolated-tests-03','isolated-clippy-02']:
 r=json.loads((root/'commands'/str(name+'.json')).read_text());assert r['exit_code']==0 and r['source_before_sha256']==r['source_after_sha256']==sha(after)
testlog=(root/'commands/isolated-tests-03.log').read_text();assert '23 passed; 0 failed' in testlog and '4 passed; 0 failed' in testlog
assert json.loads((root/'commands/render-tests-01.json').read_text())['exit_code']!=0
assert json.loads((root/'commands/isolated-tests-01.json').read_text())['exit_code']!=0
ops=[]
for label in ['master','worker']:
 with tempfile.TemporaryDirectory(prefix='zs1-125-render-'+label+'-') as raw:
  temp=Path(raw);run(['git','init','--quiet'],temp);p=temp/'src/render.rs';p.parent.mkdir();p.write_bytes(before)
  sentinel=temp/'unrelated';sentinel.write_bytes(b'preserve\x00\r\n');patch=root/'product.patch'
  stats=run(['git','apply','--numstat',str(patch)],temp).splitlines();assert len(stats)==1 and stats[0].split('\t')[2]=='src/render.rs'
  for phase,flags in [('forward_check',['--check']),('forward',[]),('reverse_check',['--reverse','--check']),('reverse',['--reverse'])]:
   if phase=='reverse_check':assert p.read_bytes()==after and sha(p.read_bytes())==sha(after)
   run(['git','apply',*flags,str(patch)],temp);ops.append(dict(baseline=label,phase=phase,passed=True))
  assert p.read_bytes()==before and sentinel.read_bytes()==b'preserve\x00\r\n'
print(json.dumps(dict(passed=True,manifest_sha256=sha((root/'manifest.json').read_bytes()),artifacts=len(expected),source_after_sha256=sha(after),patch_sha256=sha((root/'product.patch').read_bytes()),operations=ops,exact_bytes=True,sentinel_preserved=True,cargo_executions_by_verifier=0,pty_runs=0,http_runs=0,scope='offline integrity only; integration and ZS1-125 acceptance pending'),indent=2))

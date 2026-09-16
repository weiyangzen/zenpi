"""Independent offline ready verification. Only temporary Git repositories are written."""
from pathlib import Path
import json,hashlib,subprocess,tempfile,sys
E=Path(__file__).resolve().parent;root=E.parents[4]
def sha(b):return hashlib.sha256(b).hexdigest()
def run(argv,cwd):
 p=subprocess.run(argv,cwd=cwd,capture_output=True,text=True);assert p.returncode==0,(argv,p.stdout,p.stderr);return p.stdout
m=json.loads((root/'manifest.json').read_text());assert m['item']=='ZS1-097' and not m['product_changes']
expected=set(m['files']);actual={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and p!=root/'manifest.json'};assert actual==expected
for rel,r in m['files'].items():
 p=Path(rel);assert not p.is_absolute() and '..' not in p.parts
 b=(root/p).read_bytes();assert len(b)==r['bytes'] and sha(b)==r['sha256'],rel
checks=json.loads(run([sys.executable,str(E/'check_static.py')],root));assert checks['passed']
R=Path(m['learn_report']);new=(root/R).read_bytes();ops=[]
assert m['master_before']==m['worker_before']=='absent' and (root/'master.patch').read_bytes()==(root/'worker.patch').read_bytes()
for kind in ['master','worker']:
 with tempfile.TemporaryDirectory(prefix='zs1-097-offline-'+kind+'-') as raw:
  tmp=Path(raw);run(['git','init','--quiet'],tmp);target=tmp/R;assert not target.exists()
  sentinel=tmp/'unrelated-sentinel';sentinel.write_bytes(b'unchanged\x00\r\n')
  patch=root/(kind+'.patch');stat=run(['git','apply','--numstat',str(patch)],tmp).splitlines();assert len(stat)==1 and stat[0].split('\t')[2]==str(R)
  for phase,flags in [('forward_check',['--check']),('forward',[]),('reverse_check',['--reverse','--check']),('reverse',['--reverse'])]:
   if phase=='reverse_check':assert target.read_bytes()==new and sha(target.read_bytes())==sha(new)
   run(['git','apply',*flags,str(patch)],tmp);ops.append(dict(kind=kind,phase=phase,passed=True))
  assert not target.exists() and sentinel.read_bytes()==b'unchanged\x00\r\n'
print(json.dumps(dict(passed=True,manifest_sha256=sha((root/'manifest.json').read_bytes()),report_sha256=sha(new),artifacts=len(expected),patch_operations=ops,exact_report_bytes=True,reverse_returns_absent=True,sentinel_preserved=True,product_behavior_runs=0,cargo_runs=0,pty_runs=0,http_runs=0,scope='offline artifact integrity only; master semantic G-FILE/G-STAGE pending'),indent=2))

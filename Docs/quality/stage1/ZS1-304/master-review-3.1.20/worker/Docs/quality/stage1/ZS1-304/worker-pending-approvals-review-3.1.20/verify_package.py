"""Offline ready verifier. Writes only its own temporary Git repositories."""
from pathlib import Path
import json, hashlib, subprocess, tempfile, sys
E=Path(__file__).resolve().parent
root=E.parents[4]
def sha(b):return hashlib.sha256(b).hexdigest()
def run(argv,cwd):
    result=subprocess.run(argv,cwd=cwd,capture_output=True,text=True)
    assert result.returncode==0,(argv,result.stdout,result.stderr)
    return result.stdout
manifest=json.loads((root/'manifest.json').read_text())
assert manifest['item']=='ZS1-304' and manifest['product_changes'] is False
assert manifest['source_tests_executed']==0
expected=set(manifest['files'])
actual={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and p!=root/'manifest.json'}
assert actual==expected,(actual-expected,expected-actual)
for relative,record in manifest['files'].items():
    path=Path(relative)
    assert not path.is_absolute() and '..' not in path.parts
    b=(root/path).read_bytes()
    assert len(b)==record['bytes'] and sha(b)==record['sha256'],relative
structural=json.loads(run([sys.executable,str(E/'check_static.py')],root))
assert structural['passed']
R=Path(manifest['learn_report'])
new=(root/R).read_bytes()
old=(E/'history/original-worker-report.md').read_bytes()
baseline=json.loads((E/'master-baseline.json').read_text())
assert not baseline['exists'] and manifest['master_before']=='absent'
operations=[]
for label in ['master','worker']:
    patch=root/f'{label}.patch'
    with tempfile.TemporaryDirectory(prefix='zs1-304-offline-'+label+'-') as raw:
        temp=Path(raw)
        run(['git','init','--quiet'],temp)
        target=temp/R
        if label=='worker':target.parent.mkdir(parents=True);target.write_bytes(old)
        sentinel=temp/'unrelated-sentinel';sentinel.write_bytes(b'unchanged\n')
        numstat=run(['git','apply','--numstat',str(patch)],temp).splitlines()
        assert len(numstat)==1 and numstat[0].split('\t')[2]==str(R),numstat
        for phase,options in [('forward_check',['--check']),('forward',[]),('reverse_check',['--reverse','--check']),('reverse',['--reverse'])]:
            if phase=='reverse_check':assert target.read_bytes()==new
            run(['git','apply',*options,str(patch)],temp)
            operations.append(dict(delta=label,phase=phase,passed=True))
        if label=='master':assert not target.exists()
        else:assert target.read_bytes()==old
        assert sentinel.read_bytes()==b'unchanged\n'
print(json.dumps(dict(passed=True,manifest_sha256=sha((root/'manifest.json').read_bytes()),artifacts=len(expected),source_tests_read=3,source_tests_executed=0,capability_groups=10,patch_operations=operations,exact_report_bytes=True,unrelated_sentinel_preserved=True,scope='offline package integrity only; semantic G-FILE pending',cargo_runs=0,pty_runs=0,http_runs=0),ensure_ascii=False,indent=2))

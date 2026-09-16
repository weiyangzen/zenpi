from pathlib import Path
import hashlib,json,re,shutil,sys,tarfile
R=Path(__file__).resolve().parents[3];D=Path(__file__).resolve().parent;W=D/'worker'
Q=R/'Docs/quality/stage1/ZS1-125/master-approval-timer-3.1.21'
assert not Q.exists()
def read(p):return json.loads(p.read_text())
def meta(p):
 b=p.read_bytes();return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
expected=read(D/'root-final-build-after-inputs.json')
assert all(meta(R/p)==m for p,m in expected.items())
assert read(D/'root-final-result.json')['passed']
assert read(D/'root-offline.stdout.json')['passed']==671
assert read(D/'root-offline.stdout.json')['failed']==0
assert meta(W/'manifest.json')['sha256']=='dd615158393e266b72af8636bc73ae687f3d6138375a41e23686cca9acbc68e0'
for item in read(W/'manifest.json')['payload']:
 assert meta(W/item['path'])=={k:item[k] for k in ['bytes','sha256']}
for name in ['root-final-timer','root-final-canonical','root-final-regression','root-final-format','root-final-clippy','root-final-build','root-final-pty']:
 row=read(D/(name+'.run.json'));assert row['exit_code']==0 and row['log']==meta(D/(name+'.log'))
for name in ['root-before-negative','root-after-timer','root-footer-before']:
 assert read(D/(name+'.run.json'))['exit_code']==101
assert read(D/'root-pty.run.json')['exit_code']==1
assert read(D/'root-journal-review.json')['passed']
assert read(D/'root-final-process-observation.json')['no_live_pty_process']
assert '15 passed; 0 failed;' in (D/'root-final-timer.log').read_text()
counts=list(map(int,re.findall(r'test result: ok\. (\d+) passed; 0 failed',(D/'root-final-regression.log').read_text())))
assert len(counts)==11 and sum(counts)==179
pty=read(D/'root-final-pty-evidence/result.json')
assert len(pty['checks'])==14 and all(c['ok'] for c in pty['checks'])
assert pty['error'] is None and pty['exit']==0 and pty['model_requests']==0 and pty['gate_requests']==1
assert pty['journal_event_counts']['turn']==1
assert pty['binary']==meta(D/'root-final-bin/zenpi')
assert all((R/p).read_bytes()==(D/'root-final-candidate'/p).read_bytes() for p in ['src/tui.rs','tests/tui_transcript_ux.rs'])
sys.path.insert(0,str(R/'tools'));import validate_stage1_blueprint as v
bp=v.parse((R/v.BLUEPRINT).read_text());result=v.validate(R)
assert result['ok'] and bp.items['ZS1-125'].state=='[ ]'
dump(D/'root-final-blueprint-validation.json',result)
Q.mkdir(parents=True)
for p in D.iterdir():
 if p.is_file():shutil.copy2(p,Q/('review.md' if p.name=='root-review.md' else p.name))
for folder in ['root-pty-evidence','root-final-pty-evidence']:
 shutil.copytree(D/folder,Q/folder)
for folder in ['worker','main-input-snapshot','main-before','root-final-candidate']:
 with tarfile.open(Q/(folder+'.tar.gz'),'x:gz') as tar:
  for p in sorted((D/folder).rglob('*')):
   if p.is_file():tar.add(p,arcname=p.relative_to(D/folder).as_posix(),recursive=False)
with tarfile.open(Q/'final-runtime-binary.tar.gz','x:gz') as tar:
 tar.add(D/'root-final-bin/zenpi',arcname='zenpi',recursive=False)
verification={'item':'ZS1-125','complete':False,'master_accepted':False,'local_fix_integrated':True,'regression_tests':179,'distinct_library_tests':16,'total_distinct_rust_tests':195,'root_pty_checks':14,'root_pty_runs':2,'root_final_pty_exit':0,'first_root_pty_driver_exit':1,'old_failures_preserved':True,'product_files':{p:meta(R/p) for p in ['src/tui.rs','tests/tui_transcript_ux.rs']},'binary':meta(D/'root-final-bin/zenpi'),'blueprint_snapshot':bp.snapshot,'requirement_digest':bp.requirement,'scope':'Approval timer and hidden approval footer. No full125, release, startup117, entireTUI or source/target file acceptance.'}
dump(D/'master-verification.json',verification);dump(Q/'master-verification.json',verification)
dump(Q/'manifest.json',{'item':'ZS1-125','complete':False,'local_fix_integrated':True,'artifacts':{str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}})
assert all(meta(R/p)==m for p,m in expected.items())
print(json.dumps({'verification':verification,'public_manifest':meta(Q/'manifest.json')},ensure_ascii=False))

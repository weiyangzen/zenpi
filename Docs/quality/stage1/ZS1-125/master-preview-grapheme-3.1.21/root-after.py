from pathlib import Path
import subprocess,json,os,datetime,hashlib
R=Path(__file__).resolve().parents[3]
D=Path(__file__).resolve().parent
assert not (D/'after-started.json').exists()
def identity(p):
 b=p.read_bytes();return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def inputs():
 files=[p for folder in ['src','tests'] for p in (R/folder).rglob('*.rs')]+[R/'Cargo.toml',R/'Cargo.lock']
 return {str(p.relative_to(R)):identity(p) for p in files}
frozen=inputs();(D/'inputs-after.json').write_text(json.dumps(frozen,indent=2)+'\n')
(D/'after-started.json').write_text(json.dumps({'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat()})+'\n')
env=os.environ.copy();env['ZENPI_HOME']=str(D/'fixture-after');env['CARGO_BUILD_JOBS']='2'
cargo=['cargo','+stable-aarch64-apple-darwin']
commands=[('render',cargo+['test','--locked','--lib','render::tests','--','--nocapture']),('tui',cargo+['test','--locked','--test','tui_preview_graphemes','--test','tui_transcript_ux','--test','tui_approval_focus','--test','tui_bentobox','--test','view_model','--','--nocapture']),('fmt',cargo+['fmt','--all','--','--check']),('clippy',cargo+['clippy','--locked','--all-targets','--','-D','warnings']),('debug',cargo+['build','--locked','--bin','zenpi'])]
results=[]
for name,argv in commands:
 assert inputs()==frozen,'input drift before '+name
 start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with (D/(name+'.stdout.log')).open('wb') as out,(D/(name+'.stderr.log')).open('wb') as err:
  result=subprocess.run(argv,cwd=R,env=env,stdout=out,stderr=err,timeout=300)
 row={'name':name,'argv':argv,'started_at':start,'ended_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exit_code':result.returncode,'inputs_unchanged':inputs()==frozen}
 results.append(row);(D/'after-runs.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(row),flush=True)
 assert row['inputs_unchanged']
 if result.returncode:raise SystemExit(result.returncode)
(D/'binary.json').write_text(json.dumps(identity(R/'target/debug/zenpi'),indent=2)+'\n')

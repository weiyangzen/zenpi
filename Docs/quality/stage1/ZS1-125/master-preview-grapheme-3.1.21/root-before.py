from pathlib import Path
import subprocess,json,os,datetime,hashlib
R=Path(__file__).resolve().parents[3]
D=Path(__file__).resolve().parent
assert not (D/'before-run.json').exists() and not (D/'before-started.json').exists()
argv=['cargo','+stable-aarch64-apple-darwin','test','--locked','--test','tui_preview_graphemes','--','--nocapture']
start=datetime.datetime.now(datetime.timezone.utc).isoformat()
(D/'before-started.json').write_text(json.dumps({'argv':argv,'started_at':start})+'\n')
env=os.environ.copy();env['ZENPI_HOME']=str(D/'fixture-before');env['CARGO_BUILD_JOBS']='2'
with (D/'before-stdout.log').open('wb') as out,(D/'before-stderr.log').open('wb') as err:
 result=subprocess.run(argv,cwd=R,env=env,stdout=out,stderr=err,timeout=240)
(D/'before-run.json').write_text(json.dumps({'exit_code':result.returncode,'argv':argv,'started_at':start,'ended_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'tests_sha256':hashlib.sha256((R/'tests/tui_preview_graphemes.rs').read_bytes()).hexdigest(),'render_sha256':hashlib.sha256((R/'src/render.rs').read_bytes()).hexdigest()},indent=2)+'\n')
print((D/'before-stdout.log').read_text());print((D/'before-stderr.log').read_text()[-12000:]);print('exit',result.returncode)

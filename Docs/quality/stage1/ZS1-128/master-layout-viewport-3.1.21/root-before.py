from pathlib import Path
import datetime,hashlib,json,os,subprocess
R=Path(__file__).resolve().parents[3];D=Path(__file__).resolve().parent
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
prior=json.loads((D/'initial-inputs.json').read_text())
current={p:meta(R/p) for p in prior}
assert all(current[p]==prior[p] for p in prior if p!='tests/tui_bentobox.rs')
assert current['tests/tui_bentobox.rs']!=prior['tests/tui_bentobox.rs']
assert not (D/'root-before-inputs.json').exists()
(D/'root-before-inputs.json').write_text(json.dumps(current,indent=2)+'\n')
argv=['cargo','+stable-aarch64-apple-darwin','test','--locked','--offline','--test','tui_bentobox','short_workspace_','--','--nocapture']
env=os.environ.copy();env.update(CARGO_TARGET_DIR=str(R/'target'),CARGO_BUILD_JOBS='2')
start=datetime.datetime.now(datetime.timezone.utc).isoformat()
with (D/'root-before.log').open('xb') as log:
 p=subprocess.run(argv,cwd=R,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=600)
(D/'root-before.run.json').write_text(json.dumps(dict(argv=argv,started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,log=meta(D/'root-before.log')),indent=2)+'\n')
assert {p:meta(R/p) for p in current}==current
print((D/'root-before.log').read_text()[-6000:],flush=True)

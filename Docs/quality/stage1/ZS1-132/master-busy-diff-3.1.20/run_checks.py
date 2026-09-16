"""One-shot current-checkout targeted checks; refuses previous output paths."""
from pathlib import Path
import datetime,hashlib,json,os,subprocess,shutil
R=Path('/Users/wangweiyang/GitHub/zenpi');D=Path(__file__).resolve().parent
env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2')
jobs=[('tests',['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--test','headless','--test','headless_project_workspace','--test','slash']),('clippy',['cargo','+stable-aarch64-apple-darwin','clippy','--offline','--locked','--jobs','2','--all-targets','--','-D','warnings']),('fmt',['cargo','+stable-aarch64-apple-darwin','fmt','--check']),('build',['cargo','+stable-aarch64-apple-darwin','build','--offline','--locked','--jobs','2'])]
for name,argv in jobs:
    started=datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (D/(name+'.log')).open('xb') as f:
        child=subprocess.Popen(argv,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
        try:code=child.wait(timeout=300)
        except subprocess.TimeoutExpired:
            import signal
            os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=10);code=124
    rec=dict(argv=argv,cwd=str(R),started_at=started,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,log_sha256=hashlib.sha256((D/(name+'.log')).read_bytes()).hexdigest())
    (D/(name+'.run.json')).write_text(json.dumps(rec,indent=2)+'\n')
    print(name,code,flush=True)
    assert code==0,name
shutil.copy2(R/'target/debug/zenpi',D/'zenpi');(D/'zenpi').chmod(0o555)
print('binary',hashlib.sha256((D/'zenpi').read_bytes()).hexdigest(),flush=True)

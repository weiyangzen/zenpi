from pathlib import Path
import datetime,hashlib,json,os,shutil,signal,subprocess,sys
R=Path(__file__).resolve().parents[2]
D=R/'.ops/stage1_execution/escape-current-3.1.21'
D.mkdir(exist_ok=False)
def info(p):
 b=p.read_bytes();return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def run(name,args,cwd=R,timeout=300):
 env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1')
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();timed=False
 with (D/(name+'.log')).open('xb') as f:
  p=subprocess.Popen(args,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
  try:code=p.wait(timeout=timeout)
  except subprocess.TimeoutExpired:
   timed=True;os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=10);code=124
 dump(D/(name+'.run.json'),dict(argv=args,cwd=str(cwd),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=p.pid,reaped=p.poll() is not None,timed_out=timed,log=info(D/(name+'.log'))))
 print(name,code,flush=True);assert code==0,name
W=Path('/Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target129-escape-boundary-3.1.21-ready')
assert info(W/'manifest.json')['sha256']=='ee4b7c7f491a9c57312f23e5a754f706259b766fff22738e2a19938fe3d37cd0'
shutil.copytree(W,D/'worker')
run('worker-offline',['python3','-B',str(D/'worker/verify.py')])
inputs=json.loads((R/'.ops/stage1_execution/busy-test-shell-3.1.21/inputs-after.json').read_text())
before={r:info(R/r) for r in inputs};dump(D/'inputs-before.json',before)
for rel in before:
 p=D/'before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
mio='vendor/crossterm/src/event/source/unix/mio.rs'
assert (R/mio).read_bytes()==(D/'worker/before-mio.rs').read_bytes()
assert before['src/headless.rs']['sha256']=='9440492db142157995e0b479c2d31b80d9158ae624ca29d08f4d93db34e81509'
run('apply-check',['git','apply','--check',str(D/'worker/candidate.patch')])
run('apply',['git','apply',str(D/'worker/candidate.patch')])
after={r:info(R/r) for r in before};dump(D/'inputs-after.json',after)
assert [r for r in before if before[r]!=after[r]]==[mio]
assert (R/mio).read_bytes()==(D/'worker/after-mio.rs').read_bytes()
# Separate vendor target; root tests keep the existing main-checkout target identity.
base=['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--manifest-path',str(R/'vendor/crossterm/Cargo.toml'),'--target-dir',str(D/'vendor-target'),'--lib']
run('vendor-default',base+['zenpi_','--','--nocapture'])
run('vendor-libc-stream',base+['--features','libc,event-stream','zenpi_','--','--nocapture'])
run('root-tests',['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--test','tui_composer','--test','tui_bentobox','--test','layout_persistence','--test','headless_protocol','--test','headless_project_workspace','--test','slash'])
run('root-clippy',['cargo','+stable-aarch64-apple-darwin','clippy','--offline','--locked','--jobs','2','--all-targets','--','-D','warnings'])
run('root-fmt',['cargo','+stable-aarch64-apple-darwin','fmt','--check'])
assert all(info(R/r)==m for r,m in after.items()),'source drift during validation'
dump(D/'master-verification.json',dict(complete=False,product_patch='mio1afa→41321e',headless_sha256=after['src/headless.rs']['sha256'],production_release_rebuilt=False,budget_rerun=False,current_inputs=after))
print('ESC current-source integration complete; release and full-product acceptance pending.',flush=True)

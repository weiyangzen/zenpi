from pathlib import Path
import datetime,hashlib,json,os,shutil,signal,subprocess,sys
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/shutdown-current-3.1.21';D.mkdir(exist_ok=False)
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def run(name,args,cwd=R,allowed=(0,),timeout=300):
 env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1')
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();timed=False
 with (D/(name+'.log')).open('xb') as f:
  p=subprocess.Popen(args,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
  try:code=p.wait(timeout=timeout)
  except subprocess.TimeoutExpired:timed=True;os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=10);code=124
 dump(D/(name+'.run.json'),dict(argv=args,cwd=str(cwd),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=p.pid,reaped=p.poll() is not None,timed_out=timed,HOME_preserved=env.get('HOME')==os.environ.get('HOME'),CODEX_HOME_preserved=env.get('CODEX_HOME')==os.environ.get('CODEX_HOME'),log=meta(D/(name+'.log'))))
 print(name,code,flush=True);assert code in allowed,name
W=Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/stage132-shutdown321-ready');assert meta(W/'manifest.json')['sha256']=='b486ca1a9cfbac36515822a2c428382d4d301ae2ff43712887f57aa64e4511d0'
shutil.copytree(W,D/'worker');W=D/'worker'
run('worker-offline',['python3','-B',str(W/'verify.py')])
keys=json.loads((R/'.ops/stage1_execution/escape-current-3.1.21/inputs-after.json').read_text());before={r:meta(R/r) for r in keys};dump(D/'inputs-before.json',before)
for rel in before:
 p=D/'before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
assert before['src/headless.rs']['sha256']=='9440492db142157995e0b479c2d31b80d9158ae624ca29d08f4d93db34e81509'
assert before['vendor/crossterm/src/event/source/unix/mio.rs']['sha256']=='41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f'
run('apply-check',['git','apply','--check',str(W/'candidate.patch')]);run('apply',['git','apply',str(W/'candidate.patch')])
after={r:meta(R/r) for r in before};assert [r for r in before if before[r]!=after[r]]==['src/headless.rs'];assert (R/'src/headless.rs').read_bytes()==(W/'files/src/headless.rs').read_bytes();dump(D/'inputs-after.json',after)
run('root-tests',['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--test','headless_protocol','--test','headless_project_workspace','--test','runtime'])
run('root-clippy',['cargo','+stable-aarch64-apple-darwin','clippy','--offline','--locked','--jobs','2','--all-targets','--','-D','warnings'])
run('root-fmt',['cargo','+stable-aarch64-apple-darwin','fmt','--check'])
source=D/'embedding-source';source.mkdir()
for rel in after:
 p=source/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
(source/'examples').mkdir(exist_ok=True);shutil.copy2(W/'evidence/shutdown_host.rs',source/'examples/shutdown_host.rs')
dump(D/'embedding-inputs.json',{str(p.relative_to(source)):meta(p) for p in source.rglob('*') if p.is_file()})
run('embedding-build',['cargo','+stable-aarch64-apple-darwin','build','--release','--offline','--locked','--jobs','2','--target-dir',str(D/'embedding-target'),'--example','shutdown_host'],source)
shutil.copy2(D/'embedding-target/release/examples/shutdown_host',D/'shutdown-host-current');(D/'shutdown-host-current').chmod(0o555);dump(D/'embedding-binary.json',meta(D/'shutdown-host-current'))
for name,binary,allowed in [('before',W/'evidence/binaries/shutdown-host-before',(1,)),('after',D/'shutdown-host-current',(0,))]:
 run('probe-'+name,['python3','-B',str(W/'evidence/probe.py'),'--binary',str(binary),'--out',str(D/('probe-'+name))],allowed=allowed,timeout=150)
 x=json.loads((D/('probe-'+name)/'result.json').read_text());assert 'exception' not in x and len(x['checks'])==53 and sum(not a for a in x['checks'].values())==(4 if name=='before' else 0)
 assert len(x['hosts'])==10 and all(h['reaped'] and h['reader_threads_joined'] and h['exit_code']==0 for h in x['hosts'])
assert all(meta(R/r)==m for r,m in after.items()),'source drift'
print('Current shutdown integration and real embedding comparison passed; release still pending.',flush=True)

# -*- coding: utf-8 -*-
"""One-shot current-source integration with separate fresh build targets."""
from pathlib import Path
import datetime,hashlib,json,os,shutil,signal,subprocess,sys
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/busy-test-shell-3.1.21';D.mkdir(exist_ok=False)
sys.path.insert(0,str(R/'tools'));import validate_stage1_blueprint as v
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def run(name,argv,cwd=R,timeout=420,expected=0):
 env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1')
 start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with (D/(name+'.log')).open('xb') as log:
  p=subprocess.Popen(argv,cwd=cwd,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  try:code=p.wait(timeout=timeout)
  except subprocess.TimeoutExpired:
   os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=10);code=124
 dump(D/(name+'.run.json'),dict(argv=argv,cwd=str(cwd),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,expected_exit=expected,pid=p.pid,reaped=p.poll() is not None,log=meta(D/(name+'.log'))))
 print(name,code,flush=True);assert code==expected,(name,code)
assert v.validate(R)['ok']
workers={
 'worker-B':(Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-133-busy-diff-tests-ready'),'daaf4c1342bb86115a2420add98dcf92622cb5fd180f501d30c3d75180746bfb'),
 'worker-C':(Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/stage132-user-shell321-ready'),'7ffc9345fd847e13b32db3e74a37099cda8882ef208733991b8ec41e72660375')}
for name,(source,sha) in workers.items():
 assert meta(source/'manifest.json')['sha256']==sha
 shutil.copytree(source,D/name)
 run(name+'-offline',['python3','-B',str(D/name/'verify.py')],timeout=120)
inputs=json.loads((D/'worker-C/evidence/build-inputs.json').read_text())
before={rel:meta(R/rel) for rel in inputs};dump(D/'inputs-before.json',before)
for rel in before:
 p=D/'before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
assert before['src/headless.rs']['sha256']=='0ece91f883f335e677cf853af7f754d013ba8d00b834018c9044fce69e3f74e4'
assert before['tests/headless_project_workspace.rs']['sha256']=='6e87b7649b0e61c8fded9ec1b7bab953a479d6442020868a3e76d408d411248d'
for name in workers:run(name+'-apply-check',['git','apply','--check',str(D/name/'candidate.patch')])
for name in workers:run(name+'-apply',['git','apply',str(D/name/'candidate.patch')])
after={rel:meta(R/rel) for rel in inputs}
assert {rel for rel in inputs if before[rel]!=after[rel]}=={'src/headless.rs','tests/headless_project_workspace.rs'}
assert (R/'src/headless.rs').read_bytes()==(D/'worker-C/files/src/headless.rs').read_bytes()
assert (R/'tests/headless_project_workspace.rs').read_bytes()==(D/'worker-B/files/tests/headless_project_workspace.rs').read_bytes()
dump(D/'inputs-after.json',after)
work=D/'embedding-source';work.mkdir()
for rel in after:
 p=work/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
host=work/'examples/shell_host.rs';host.parent.mkdir(exist_ok=True);shutil.copy2(D/'worker-C/evidence/shell_host.rs',host)
dump(D/'embedding-inputs.json',{**after,'examples/shell_host.rs':meta(host)})
target=D/'root-target';assert not target.exists()
run('root-tests',['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--target-dir',str(target),'--test','headless_project_workspace','--test','headless_protocol','--','--nocapture'])
run('root-clippy',['cargo','+stable-aarch64-apple-darwin','clippy','--offline','--locked','--jobs','2','--target-dir',str(target),'--all-targets','--','-D','warnings'])
run('root-fmt',['cargo','+stable-aarch64-apple-darwin','fmt','--check'])
target=D/'embedding-target';assert not target.exists()
run('embedding-build',['cargo','+stable-aarch64-apple-darwin','build','--release','--offline','--locked','--jobs','2','--target-dir',str(target),'--example','shell_host'],cwd=work)
shutil.copy2(target/'release/examples/shell_host',D/'shell-host-current');(D/'shell-host-current').chmod(0o555)
dump(D/'embedding-binary.json',meta(D/'shell-host-current'))
for name,binary,code in [('shell-before',D/'worker-C/evidence/binaries/shell-host-before',1),('shell-after',D/'shell-host-current',0)]:
 run(name,['python3','-B',str(D/'worker-C/evidence/probe.py'),'--binary',str(binary),'--out',str(D/name)],timeout=150,expected=code)
 d=json.loads((D/name/'result.json').read_text())
 assert 'exception' not in d and len(d['checks'])==52 and sum(not x for x in d['checks'].values())==(6 if code else 0)
 assert d['all_shell_pids_gone'] and len(d['hosts'])==5 and all(h['reaped'] and h['reader_threads_joined'] and h['exit_code']==0 for h in d['hosts'])
assert all(meta(R/p)==m for p,m in after.items()),'current-source drift during checks'
print('current combination: workspace/protocol tests, clippy/fmt, actual shell before6fails/after52pass; no production release/budget rerun',flush=True)

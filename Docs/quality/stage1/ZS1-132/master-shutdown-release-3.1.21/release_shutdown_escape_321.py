from pathlib import Path
import datetime,hashlib,json,os,shutil,signal,subprocess
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/shutdown-current-3.1.21';H=D/'release-hosts';H.mkdir(exist_ok=False)
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def run(name,args,allowed=(0,),timeout=300,clean_proxy=False):
 env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1');removed=[]
 if clean_proxy:
  removed=[k for k in env if k.lower() in ['http_proxy','https_proxy','all_proxy','no_proxy']]
  for k in removed:env.pop(k)
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();timed=False
 with (H/(name+'.log')).open('xb') as f:
  p=subprocess.Popen(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
  try:code=p.wait(timeout=timeout)
  except subprocess.TimeoutExpired:timed=True;os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=10);code=124
 dump(H/(name+'.run.json'),dict(argv=args,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=p.pid,reaped=p.poll() is not None,timed_out=timed,removed_proxy_names=removed,HOME_preserved=env.get('HOME')==os.environ.get('HOME'),CODEX_HOME_preserved=env.get('CODEX_HOME')==os.environ.get('CODEX_HOME'),log=meta(H/(name+'.log'))))
 print(name,code,flush=True);assert code in allowed,name;return code
assert json.loads((D/'probe-after.run.json').read_text())['exit_code']==0
after=json.loads((D/'inputs-after.json').read_text());assert all(meta(R/p)==m for p,m in after.items())
# The harness builds this production revision, then its first launch is sample0.
# Exactly3 samples and original thresholds; a failure is preserved and not retried.
code=run('budget',['python3','tools/bench_runtime.py','--samples','3','--output',str(H/'budget.json'),'--startup-evidence-dir',str(H/'startup')],allowed=(0,1))
b=json.loads((H/'budget.json').read_text());assert code==(0 if b['ok'] else 1)
shutil.copy2(R/'target/release/zenpi',H/'zenpi-release');(H/'zenpi-release').chmod(0o555);dump(H/'binary.json',meta(H/'zenpi-release'));assert b['cold_start']['binary_sha256']==meta(H/'zenpi-release')['sha256']
run('project-plus',['python3','tools/tui_project_workspace_smoke.py','--binary',str(H/'zenpi-release'),'--evidence',str(H/'project-plus.json')],timeout=180,clean_proxy=True)
p=json.loads((H/'project-plus.json').read_text());assert p['status']=='passed' and p['real_pty'] and len(p['assertions'])==12 and p['binary_sha256']==b['cold_start']['binary_sha256']
run('kill-yank',['python3','tools/tui_composer_smoke.py','--binary',str(H/'zenpi-release'),'--kill-yank-only','--evidence',str(H/'kill-yank.json')],timeout=180,clean_proxy=True)
p=json.loads((H/'kill-yank.json').read_text());assert p['passed'] and len(p['checks'])==29 and all(p['checks'].values()) and p['request_count']==4 and p['tui_processes']==3 and p['binary_sha256']==b['cold_start']['binary_sha256']
assert all(meta(R/p)==m for p,m in after.items()),'input drift'
print(json.dumps(dict(budget_ok=b['ok'],gates=b['gates'],samples=b['cold_start']['elapsed_ms']['samples'],binary=meta(H/'zenpi-release'),project_PTY_assertions=12,kill_yank_PTY_checks=29)),flush=True)

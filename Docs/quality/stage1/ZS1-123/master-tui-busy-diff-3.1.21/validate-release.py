from pathlib import Path
import datetime, hashlib, json, os, shutil, signal, subprocess
R=Path(__file__).resolve().parents[2]
I=R/'.ops/stage1_execution/tui-busy-diff-integration-3.1.21'
D=R/'.ops/stage1_execution/tui-busy-diff-release-3.1.21'
assert not D.exists();D.mkdir()
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def save(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
inputs=json.loads((I/'inputs-final-after-validation.json').read_text())
assert all(meta(R/p)==x for p,x in inputs.items())
save(D/'inputs.json',inputs)
def run(name,argv,allowed=(0,),timeout=600):
 env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1')
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();timed=False
 with (D/(name+'.log')).open('xb') as out:
  child=subprocess.Popen(argv,cwd=R,env=env,stdout=out,stderr=subprocess.STDOUT,start_new_session=True)
  try:code=child.wait(timeout=timeout)
  except subprocess.TimeoutExpired:
   timed=True;os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=10);code=124
 save(D/(name+'.run.json'),dict(argv=argv,started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=child.pid,reaped=child.poll() is not None,timed_out=timed,log=meta(D/(name+'.log')),HOME_preserved=env.get('HOME')==os.environ.get('HOME'),CODEX_HOME_preserved=env.get('CODEX_HOME')==os.environ.get('CODEX_HOME')))
 assert all(meta(R/p)==x for p,x in inputs.items()),'input drift'
 print(name,code,flush=True);assert code in allowed,name
 return code
code=run('budget',['python3','tools/bench_runtime.py','--samples','3','--output',str(D/'budget.json'),'--startup-evidence-dir',str(D/'startup')],allowed=(0,1))
budget=json.loads((D/'budget.json').read_text());assert code==(0 if budget['ok'] else 1)
binary=D/'zenpi-release';shutil.copy2(R/'target/release/zenpi',binary);binary.chmod(0o555)
assert meta(binary)==meta(I/'zenpi-release')
assert meta(binary)['sha256']==budget['cold_start']['binary_sha256'];save(D/'binary.json',meta(binary))
for case,script,count in [('local-diff','root_busy_diff.py',9),('slow-git','root_slow_diff.py',10)]:
 run(case,['python3',str(I/'root-pty'/script),str(binary),str(D/case)],timeout=180)
 result=json.loads((D/case/'result.json').read_text());assert result['status']=='passed' and len(result['checks'])==count and all(result['checks'].values()) and result['binary_sha256']==meta(binary)['sha256']
for case in ['projects','bentobox']:
 run(case,['python3','tools/stage1_host_smoke.py','--binary',str(binary),'--case',case,'--report-dir',str(D/case)],timeout=300)
 result=json.loads((D/case/'manifest.json').read_text());assert result['status']=='passed' and result['binary_sha256']==meta(binary)['sha256']
save(D/'master-verification.json',dict(complete=False,scope='TUI busy local diff/review plus root status-truncation repair; not whole123 or Stage1',current_binary=meta(binary),input_count=len(inputs),input_stable=True,budget_ok=budget['ok'],gates=budget['gates'],startup_samples_ms=budget['cold_start']['elapsed_ms']['samples'],rust_passed=107,pty_checks=42,pty_cases=['local-diff','slow-git','projects','bentobox']))
print(json.dumps(json.loads((D/'master-verification.json').read_text())),flush=True)

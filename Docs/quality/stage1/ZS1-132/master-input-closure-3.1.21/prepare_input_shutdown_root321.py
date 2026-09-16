from pathlib import Path
import datetime, hashlib, json, os, shutil, signal, subprocess
R=Path(__file__).resolve().parents[2]
D=R/'.ops/stage1_execution/input-shutdown-current-3.1.21'
W=Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/stage132-input-ticket321')
assert not D.exists()
def meta(p):
    b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,data):p.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n')
test=R/'tests/headless_input_shutdown.rs'
assert not test.exists()
assert meta(R/'src/headless.rs')['sha256']=='521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0'
assert meta(W/'headless_input_shutdown.rs')['sha256']=='5e4c55fe667930ecef2229f19b0897c4a4c50e42012301d768118d44d20969f5'
D.mkdir()
keys=json.loads((R/'.ops/stage1_execution/blueprint-version-current-3.1.21/inputs-after.json').read_text())
before={rel:meta(R/rel) for rel in keys};dump(D/'inputs-before.json',before)
for rel in before:
    p=D/'before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
for name in ['candidate.patch','headless_input_shutdown.rs','probe.py','ticket_host.rs']:
    p=D/'preliminary-worker'/name;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(W/name,p)
shutil.copy2(W/'headless_input_shutdown.rs',test)
dump(D/'test-binding.json',dict(test=meta(test),headless=meta(R/'src/headless.rs'),worker_status='active; only previously fully reviewed test installed; product patch not applied'))
argv=['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--test','headless_input_shutdown','--','--test-threads=1']
env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1');env.pop('ZENPI_DOMAIN_STORE',None)
start=datetime.datetime.now(datetime.timezone.utc).isoformat();timed=False
with (D/'root-before.log').open('xb') as out:
    p=subprocess.Popen(argv,cwd=R,env=env,stdout=out,stderr=subprocess.STDOUT,start_new_session=True)
    try:code=p.wait(timeout=300)
    except subprocess.TimeoutExpired:
        timed=True;os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=10);code=124
dump(D/'root-before.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=p.pid,reaped=p.poll() is not None,timed_out=timed,log=meta(D/'root-before.log')))
after={rel:meta(R/rel) for rel in keys};dump(D/'inputs-after-baseline.json',after)
assert before==after,'source drift during baseline test'
assert code==101,code
log=(D/'root-before.log').read_text()
assert 'test result: FAILED. 2 passed; 1 failed;' in log
assert 'missing/duplicate terminal' in log
assert 'shutdown grace changed:' not in log
print('Current root521c:2 controls pass; delayed input lacks terminal. New regression installed; product patch still pending final sealed review.',flush=True)

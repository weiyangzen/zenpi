# -*- coding: utf-8 -*-
"""One-shot integration of two independently reviewed, non-overlapping patches."""
from pathlib import Path
import datetime, hashlib, json, os, shutil, signal, subprocess, sys
R = Path(__file__).resolve().parents[2]
D = R / '.ops/stage1_execution/readiness-identity-3.1.21'
D.mkdir(exist_ok=False)
sys.path.insert(0, str(R/'tools'))
import validate_stage1_blueprint as v
def info(p):
    b=p.read_bytes(); return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x): p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def run(name,argv,timeout=120):
    start=datetime.datetime.now(datetime.timezone.utc).isoformat()
    env=os.environ.copy();env.update(PYTHONDONTWRITEBYTECODE='1',CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2')
    with (D/(name+'.log')).open('xb') as log:
        p=subprocess.Popen(argv,cwd=R,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try: code=p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=10);code=124
    dump(D/(name+'.run.json'),dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=p.pid,reaped=p.poll() is not None,log=info(D/(name+'.log'))))
    print(name,code,flush=True);assert code==0,name
bp=v.parse((R/v.BLUEPRINT).read_text());assert bp.header['blueprint_version']=='3.1.21' and v.validate(R)['ok']
workers={
 'worker-A':Path('/Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target129-ctrl-u-readiness-3.1.20-ready'),
 'worker-C':Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/stage132-response-identity320-ready')}
expected={'worker-A':'1451782d0282b1c8ffb66f5812191b290bcfb93ef730225db29a58945785e738','worker-C':'f1f33feb2ed33ee3bf3302a1f32cbb6eb75978a81951879eee9261bbefefbb2c'}
for name,p in workers.items():
    assert info(p/'manifest.json')['sha256']==expected[name]
    shutil.copytree(p,D/name)
    run(name+'-offline',['python3','-B',str(D/name/'verify.py')])
base=json.loads((D/'worker-C/evidence/build-inputs.json').read_text())
inputs={}
for rel in base:
    p=R/rel;inputs[rel]=info(p);out=D/'before'/rel;out.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,out)
dump(D/'inputs-before.json',inputs)
changes={'src/headless.rs':('2f0835f33fb831f4ba388289df2ce5505089b333034b07cfcab57ce79b1d35c7',D/'worker-C/files/src/headless.rs'),
'vendor/crossterm/src/event/source/unix/mio.rs':('57f4a90b828444fcc6dd7199a73907bbbccf2df4b7808f9e8a0a4b0771e12e39',D/'worker-A/after-mio.rs'),
'tests/tui_composer.rs':('656e328e278861918502174ac51a56016bd205afbd360cdd955ab6550602c3f7',D/'worker-A/after-tests.rs')}
for rel,(sha,_) in changes.items():assert inputs[rel]['sha256']==sha,rel
for name in workers:run(name+'-apply-check',['git','apply','--check',str(D/name/'candidate.patch')])
for name in workers:run(name+'-apply',['git','apply',str(D/name/'candidate.patch')])
after={rel:info(R/rel) for rel in inputs}
assert {rel for rel in inputs if inputs[rel]!=after[rel]}==set(changes)
for rel,(_,p) in changes.items():assert (R/rel).read_bytes()==p.read_bytes()
dump(D/'inputs-after.json',after)
dump(D/'source-drift.json',{name:{rel:dict(worker=meta,current=inputs.get(rel)) for rel,meta in json.loads((D/'worker-C/evidence/build-inputs.json').read_text()).items() if meta!=inputs.get(rel)} for name in ['worker-C']})
run('tests',['cargo','+stable-aarch64-apple-darwin','test','--offline','--locked','--jobs','2','--test','tui_composer','--test','tui_bentobox','--test','layout_persistence','--test','headless_protocol','--test','headless_project_workspace','--test','slash'],300)
run('clippy',['cargo','+stable-aarch64-apple-darwin','clippy','--offline','--locked','--jobs','2','--all-targets','--','-D','warnings'],300)
run('fmt',['cargo','+stable-aarch64-apple-darwin','fmt','--check'],120)
run('release',['cargo','+stable-aarch64-apple-darwin','build','--release','--offline','--locked','--jobs','2'],300)
shutil.copy2(R/'target/release/zenpi',D/'zenpi-release');(D/'zenpi-release').chmod(0o555)
dump(D/'binary.json',info(D/'zenpi-release'))
assert all(info(R/rel)==meta for rel,meta in after.items()),'product/build input drift'
print('frozen release',json.dumps(info(D/'zenpi-release')),flush=True)

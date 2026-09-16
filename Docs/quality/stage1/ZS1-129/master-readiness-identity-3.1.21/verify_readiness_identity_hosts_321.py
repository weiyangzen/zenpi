# -*- coding: utf-8 -*-
"""One fresh budget run, unchanged PTY comparison and real JSONL comparison."""
from pathlib import Path
import datetime, hashlib, json, os, signal, subprocess, tarfile
R=Path(__file__).resolve().parents[2]
D=R/'.ops/stage1_execution/readiness-identity-3.1.21'
H=D/'hosts';H.mkdir(exist_ok=False)
def info(p):
    b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def run(name,argv,expected,timeout=150,clean_proxy=False):
    env=os.environ.copy();env.update(CARGO_NET_OFFLINE='true',CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1')
    removed=[]
    if clean_proxy:
        removed=[k for k in env if k.lower() in {'http_proxy','https_proxy','all_proxy','no_proxy'}]
        for k in removed:env.pop(k)
    start=datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (H/(name+'.log')).open('xb') as log:
        child=subprocess.Popen(argv,cwd=R,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try:code=child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=10);code=124
    dump(H/(name+'.run.json'),dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,pid=child.pid,reaped=child.poll() is not None,removed_proxy_names=sorted(removed),HOME_preserved=env.get('HOME')==os.environ.get('HOME'),CODEX_HOME_preserved=env.get('CODEX_HOME')==os.environ.get('CODEX_HOME'),log=info(H/(name+'.log'))))
    print(name,code,flush=True);assert code in expected,(name,code)
    return code
binary=D/'zenpi-release';assert info(binary)==json.loads((D/'binary.json').read_text())
assert info(R/'target/release/zenpi')==info(binary)
# This product revision has not been launched before this fixed 3-sample gate.
# A failing cold first sample is retained and never retried to obtain green.
budget=run('budget',['python3','tools/bench_runtime.py','--samples','3','--output',str(H/'budget.json'),'--startup-evidence-dir',str(H/'startup')],{0,1},300)
report=json.loads((H/'budget.json').read_text());assert report['cold_start']['binary_sha256']==info(binary)['sha256']
assert info(R/'target/release/zenpi')==info(binary)
assert budget==(0 if report['ok'] else 1)
with tarfile.open(D/'worker-A/binaries.tar.gz','r:gz') as tar:
    member=tar.getmember('before-release');assert member.isfile()
    with (H/'zenpi-original-readiness').open('xb') as f:f.write(tar.extractfile(member).read())
old=H/'zenpi-original-readiness';old.chmod(0o555)
assert info(old)['sha256']=='e9ba9476f7699d715ad3b0b5fe4a07aeee9f280508d3cb495d59cfa94fff6449'
for name,exe,expected in [('pty-before',old,{1}),('pty-after',binary,{0})]:
    run(name,['python3','tools/tui_composer_smoke.py','--binary',str(exe),'--kill-yank-only','--evidence',str(H/(name+'.json'))],expected,150,True)
    result=json.loads((H/(name+'.json')).read_text())
    assert result['binary_sha256']==info(exe)['sha256']
    assert len(result['checks'])==(11 if name=='pty-before' else 29)
    assert result['passed']==(name=='pty-after')
    if name=='pty-after':assert result['request_count']==4 and result['tui_processes']==3 and all(result['checks'].values())
for name,exe,expected in [('jsonl-before',D/'worker-C/evidence/binaries/zenpi-before',{1}),('jsonl-after',binary,{0})]:
    run(name,['python3','-B',str(D/'worker-C/evidence/probe.py'),'--binary',str(exe),'--out',str(H/name)],expected)
    result=json.loads((H/name/'result.json').read_text())
    assert 'exception' not in result and result['host_exit']==0 and result['host_reaped'] and result['reader_threads_joined'] and result['server_thread_joined']
    assert len(result['checks'])==35 and sum(not x for x in result['checks'].values())==(12 if name=='jsonl-before' else 0)
    assert len(json.loads((H/name/'http.json').read_text()))==1
print('budget:',report['ok'],'cold samples:',report['cold_start']['elapsed_ms']['samples'],'PTY 29 and JSONL 35 passed; old failures preserved',flush=True)

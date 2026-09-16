"""One fresh real-host before/after regression; preserve every raw output."""
from pathlib import Path
import datetime,hashlib,json,os,signal,subprocess
R=Path('/Users/wangweiyang/GitHub/zenpi');D=Path(__file__).resolve().parent
cases=[('hosts-before',D/'worker/evidence/binaries/zenpi-matching-before',1),('hosts-after',D/'zenpi',0)]
for name,binary,expected in cases:
    assert not (D/name).exists()
    argv=['python3',str(D/'worker/evidence/probe.py'),'--binary',str(binary),'--out',str(D/name)]
    start=datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (D/(name+'.log')).open('xb') as f:
        child=subprocess.Popen(argv,cwd=R,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
        try:code=child.wait(timeout=120)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=10);code=124
    (D/(name+'.run.json')).write_text(json.dumps(dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=code,expected_exit=expected,log_sha256=hashlib.sha256((D/(name+'.log')).read_bytes()).hexdigest()),indent=2)+'\n')
    assert code==expected,(name,code)
    r=json.loads((D/name/'result.json').read_text())
    assert 'exception' not in r and r['host_exit']==0 and r['host_reaped'] and r['reader_threads_joined'] and r['server_thread_joined']
    failed=[key for key,ok in r['checks'].items() if not ok]
    assert failed==(['B-before-new','B-after-new','B-after-switch-back','B-before-cancel'] if expected else [])
    assert len(r['checks'])==17
    print(name,code,'checks',len(r['checks']),'failed',failed,flush=True)

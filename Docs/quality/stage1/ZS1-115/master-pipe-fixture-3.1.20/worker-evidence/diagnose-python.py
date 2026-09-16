from pathlib import Path
import subprocess,time,json,datetime,hashlib,os,signal
P=Path(__file__).resolve().parent;D=P/'startup-diagnostic';case=D/'python';case.mkdir(exist_ok=False);(case/'pipe-mode').write_text('stdout');argv=['/usr/bin/python3',str(D/'endpoint.py')];m=dict(argv=argv,cwd=str(case),started_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),status='running');(D/'python-first.run.json').write_text(json.dumps(m,indent=2)+'\n');start=time.monotonic()
proc=subprocess.Popen(argv,cwd=case,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True);ready=False
while time.monotonic()-start<.8:
 if (case/'escaped.pid').exists():ready=True;break
 if proc.poll() is not None:break
 time.sleep(.002)
m.update(elapsed_to_observation=time.monotonic()-start,escaped_ready=ready,pid=proc.pid)
if proc.poll() is None:proc.kill()
if (case/'escaped.pid').exists():os.kill(int((case/'escaped.pid').read_text()),signal.SIGKILL)
out,err=proc.communicate(timeout=3);raw=out+err;(D/'python-first.log').write_bytes(raw);m.update(exit_code=proc.returncode,phases=(case/'phases.log').read_text() if (case/'phases.log').exists() else '',status='finished',log_sha256=hashlib.sha256(raw).hexdigest());(D/'python-first.run.json').write_text(json.dumps(m,indent=2)+'\n');print(json.dumps(m,indent=2))
# First diagnostic's Perl stage aborted during cleanup after writing escaped-ready.
(D/'perl-first-incomplete.json').write_text(json.dumps(dict(status='incomplete; cleanup exception is not a pass',argv=['/usr/bin/perl',str(D/'endpoint.pl')],error='PermissionError [Errno 1] from os.killpg after observing readiness',exit_code_unavailable=True,phases=(D/'perl/phases.log').read_text(),note='No rerun. Its escaped holder had a fixed ten-second lifetime. Native first result was already recorded before this exception.'),indent=2)+'\n')

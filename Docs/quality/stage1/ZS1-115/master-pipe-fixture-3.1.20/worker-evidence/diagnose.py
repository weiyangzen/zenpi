from pathlib import Path
import subprocess,json,hashlib,time,datetime,os,signal
P=Path(__file__).resolve().parent
source=(P/'before.rs').read_text().split('const SOURCE: &str = r#"',1)[1].split('"#;',1)[0]
D=P/'startup-diagnostic';D.mkdir(exist_ok=False)
rows=[]
def run(name,argv,cwd,limit=30):
 start=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat();result=subprocess.run(argv,cwd=cwd,capture_output=True,timeout=limit);raw=result.stdout+result.stderr;(D/(name+'.log')).write_bytes(raw);row=dict(name=name,argv=list(map(str,argv)),cwd=str(cwd),started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=result.returncode,log=name+'.log',log_sha256=hashlib.sha256(raw).hexdigest());rows.append(row);(D/'commands.json').write_text(json.dumps(rows,indent=2)+'\n');return result
(D/'endpoint.c').write_text(source)
run('compile',['cc','-O2','-Wall','-Wextra','-Werror',str(D/'endpoint.c'),'-o',str(D/'native')],D)
perl=r'''use strict; use warnings; use POSIX qw(setsid); use Time::HiRes qw(clock_gettime CLOCK_MONOTONIC sleep);
sub phase { open my $f, '>>', 'phases.log' or die $!; printf $f "%.9f pid=%d %s\n", clock_gettime(CLOCK_MONOTONIC), $$, $_[0]; close $f or die $!; }
sub pidfile {open my $f, '>', $_[0] or die $!; print $f $$;close $f or die $!;}
phase('interpreter-parent-entry');pidfile('parent.pid');my $pid=fork();defined $pid or die $!;
if ($pid==0) {setsid()>=0 or die $!;close STDIN;phase('holder-entry');pidfile('escaped.pid');phase('escaped-ready');sleep(10);POSIX::_exit(0);}
while (!-e 'escaped.pid') {sleep(.001);}POSIX::_exit(0);
'''
(D/'endpoint.pl').write_text(perl)
py="""import os,time

def phase(name):
 with open('phases.log','a') as f:f.write(f'{time.monotonic():.9f} pid={os.getpid()} {name}\\n')
def pidfile(name):
 with open(name,'w') as f:f.write(str(os.getpid()))
phase('interpreter-parent-entry');pidfile('parent.pid')
if os.fork()==0:
 os.setsid();os.close(0);phase('holder-entry');pidfile('escaped.pid');phase('escaped-ready');time.sleep(10);os._exit(0)
while not os.path.exists('escaped.pid'):time.sleep(.001)
os._exit(0)
"""
(D/'endpoint.py').write_text(py)
# One first execution per implementation: no warmup or retry, same 800 ms observation bound.
for name,argv in [('native',[str(D/'native')]),('perl',['/usr/bin/perl',str(D/'endpoint.pl')]),('python',['/usr/bin/python3',str(D/'endpoint.py')])]:
 d=D/name if name!='native' else D/'native-case';d.mkdir();(d/'pipe-mode').write_text('stdout');start=time.monotonic();proc=subprocess.Popen(argv,cwd=d,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True);ready=False
 while time.monotonic()-start<.8:
  if (d/'escaped.pid').exists():ready=True;break
  if proc.poll() is not None:break
  time.sleep(.002)
 observed=time.monotonic()-start
 try:os.killpg(proc.pid,signal.SIGKILL)
 except ProcessLookupError:pass
 if (d/'escaped.pid').exists():
  try:os.kill(int((d/'escaped.pid').read_text()),signal.SIGKILL)
  except ProcessLookupError:pass
 stdout,stderr=proc.communicate(timeout=3);raw=stdout+stderr;(D/(name+'-first.log')).write_bytes(raw)
 row=dict(name=name+'-first',argv=argv,cwd=str(d),elapsed_to_observation=observed,escaped_ready=ready,exit_code=proc.returncode,phases=(d/'phases.log').read_text() if (d/'phases.log').exists() else '',log=name+'-first.log',log_sha256=hashlib.sha256(raw).hexdigest());rows.append(row);(D/'commands.json').write_text(json.dumps(rows,indent=2)+'\n')
print(json.dumps(rows,indent=2))

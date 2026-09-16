#!/usr/bin/env python3
"""Five bounded diagnostic launches; never replaces the original budget receipt."""
import hashlib,json,os,pathlib,selectors,shutil,subprocess,tempfile,time
ROOT=pathlib.Path('/Users/wangweiyang/GitHub/zenpi')
OUT=pathlib.Path(__file__).resolve().parent
BINARY=ROOT/'target/release/zenpi'
EXPECTED='7a374a3b0764c7a4db9b3b53e36dc07d3d8b17b80f5711805a932a2090e60774'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
assert sha(BINARY)==EXPECTED
RUN=OUT/'observations';RUN.mkdir()
fixture=pathlib.Path(tempfile.mkdtemp(prefix='zenpi-bench-'))
env=os.environ.copy();env.update(ZENPI_HOME=str(fixture/'zenpi'),ZENPI_BACKEND='openai',ZENPI_BASE_URL='http://127.0.0.1:1',ZENPI_MODEL='benchmark-no-network',ZENPI_API_KEY='benchmark-placeholder')
save(RUN/'identity.json',{'binary':str(BINARY),'sha256':EXPECTED,'cwd':str(ROOT),'fixture':str(fixture),'clock':str(time.get_clock_info('perf_counter')),'samples_fixed_before_run':5,'timeout_seconds':5,'workload':'original shutdown; one shared initially absent ZENPI_HOME; unique session each round','observation_change':'Popen + selector read timestamps instead of subprocess.run; time(1) unchanged; not a gate rerun','environment_overrides':{k:env[k] for k in ['ZENPI_HOME','ZENPI_BACKEND','ZENPI_BASE_URL','ZENPI_MODEL','ZENPI_API_KEY']},'other_env_names_only':sorted(k for k in env if k.startswith(('ZENPI','OPENAI','ANTHROPIC','DYLD','RUST')) and k not in ['ZENPI_HOME','ZENPI_BACKEND','ZENPI_BASE_URL','ZENPI_MODEL','ZENPI_API_KEY']),'loadavg_before':os.getloadavg(),'wall_started_ns':time.time_ns()})
allrows=[]
for i in range(5):
 d=RUN/f'round-{i}';d.mkdir();session=fixture/f'session-{i}.jsonl';payload=b'{"type":"shutdown","id":"bench-stop"}\n'
 cmd=['/usr/bin/time','-l',str(BINARY),'--mode','headless','--session',str(session)]
 marks=[];streams={'stdout':bytearray(),'stderr':bytearray()}
 def mark(label,**kw):marks.append({'label':label,'monotonic_ns':time.perf_counter_ns(),**kw})
 mark('parent_launch_begin');start=marks[-1]['monotonic_ns']
 p=subprocess.Popen(cmd,cwd=ROOT,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 mark('wrapper_Popen_returned',wrapper_pid=p.pid)
 p.stdin.write(payload);p.stdin.close();mark('shutdown_input_supplied')
 sel=selectors.DefaultSelector()
 for name in streams:sel.register(getattr(p,name),selectors.EVENT_READ,name)
 timeout=False
 while sel.get_map():
  if time.perf_counter_ns()-start>5_000_000_000:
   timeout=True;p.kill();mark('timeout_wrapper_killed');break
  for key,_ in sel.select(timeout=.05):
   data=os.read(key.fileobj.fileno(),65536)
   if data:
    if not streams[key.data]:mark(key.data+'_first_bytes_observed')
    streams[key.data].extend(data);mark(key.data+'_chunk_observed',bytes=len(data))
   else:mark(key.data+'_eof_observed');sel.unregister(key.fileobj)
 if timeout:
  more=p.communicate()
  for name,data in zip(streams,more):streams[name].extend(data or b'')
 p.wait();mark('wrapper_exit_reaped',returncode=p.returncode);end=time.perf_counter_ns()
 # All evidence writes, fixture copying and RSS parsing happen after the measured end.
 for name,data in streams.items():(d/(name+'.raw')).write_bytes(data)
 (d/'stdin.raw').write_bytes(payload)
 rss=[]
 for line in streams['stderr'].decode(errors='replace').splitlines():
  if 'maximum resident set size' in line:rss.append(int(line.strip().split()[0]))
 row={'index':i,'command':cmd,'marks':marks,'parent_capture_end_ns':end,'elapsed_ms':(end-start)/1e6,'returncode':p.returncode,'timeout':timeout,'success_frame':b'"success":true' in streams['stdout'],'peak_rss_bytes':rss,'unavailable_boundaries':['successful target exec','Rust main/core entry','configuration completion','session initialization completion','headless entry','target exit (distinct from wrapper exit)']}
 shutil.copytree(fixture,d/'fixture-after');save(d/'receipt.json',row);allrows.append(row)
 for f in [p.stdout,p.stderr]:f.close()
 sel.close()
save(RUN/'summary.json',{'rounds':allrows,'binary_sha_after':sha(BINARY),'loadavg_after':os.getloadavg(),'original_gate':'FAIL 1208.732708ms > 1000ms; unchanged','diagnostic_only':True})
print(json.dumps({'elapsed_ms':[r['elapsed_ms'] for r in allrows],'success':[r['success_frame'] for r in allrows],'path':str(RUN)}))

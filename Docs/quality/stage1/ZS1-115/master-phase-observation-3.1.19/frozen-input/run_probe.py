from pathlib import Path
import datetime,hashlib,json,os,re,subprocess,tempfile
root=Path.cwd();r=root/'.ops/ux130-phase-observation';binary=r/'phase_probe';h=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();rows=[]
with tempfile.TemporaryDirectory(prefix='phase-observation-',dir=r) as folder:
 private=Path(folder)
 cases=[('before-watchdog','--before','watchdog',1),('after-watchdog','--after','watchdog',1),('normal','--after','normal',0),('nonzero','--after','nonzero',1),('unwind','--unwind','unused',1),('boundary','--snapshot','unused',0),('oversize','--snapshot','unused',0),('missing','--snapshot','unused',0),('fifo','--snapshot','unused',0),('symlink','--snapshot','unused',0)]
 for name,mode,child_mode,expected in cases:
  ext=private/name;ext.mkdir(mode=0o700)
  if name in ['boundary','oversize']:(ext/'phases.log').write_bytes(b'p'*(8192 if name=='boundary' else 9000))
  if name=='fifo':os.mkfifo(ext/'phases.log',0o600)
  if name=='symlink':
   sentinel=private/'external-sentinel';sentinel.write_text('DO_NOT_FOLLOW_PRIVATE_SENTINEL');(ext/'phases.log').symlink_to(sentinel)
  argv=[str(binary),mode,str(ext),child_mode];env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};row=dict(name=name,argv=argv,cwd=str(root),started=datetime.datetime.now(datetime.timezone.utc).isoformat(),binary_sha256=h(binary),expected_exit=expected)
  process=subprocess.run(argv,cwd=root,env=env,capture_output=True,text=True,timeout=6);row.update(exit=process.returncode,ended=datetime.datetime.now(datetime.timezone.utc).isoformat(),fixture_directory_removed=not ext.exists());(r/f'{name}.stdout').write_text(process.stdout);(r/f'{name}.stderr').write_text(process.stderr);row['stdout_sha256']=h(r/f'{name}.stdout');row['stderr_sha256']=h(r/f'{name}.stderr')
  rows.append(row);(r/'probe-runs.json').write_text(json.dumps(rows,indent=2)+'\n')
  assert process.returncode==expected,(name,process.returncode,process.stderr)
  if 'watchdog' in name:
   assert 'outer_timeout=true' in process.stdout and 'SIGKILL' in process.stdout and 'observed_fixture_success=false' in process.stdout
   assert re.search(r'phase_file_before_cleanup_bytes=[1-9][0-9]*',process.stdout) and not ext.exists()
   if name.startswith('before'):assert 'fixture_phase_snapshot' not in process.stdout and 'helper-entry' not in process.stdout
   else:assert 'fixture_phase_snapshot' in process.stdout and 'helper-entry' in process.stdout and 'intentional-watchdog-stall' in process.stdout
  elif name in ['normal','nonzero']:
   assert 'outer_timeout=false' in process.stdout and 'fixture_phase_snapshot' in process.stdout and 'helper-entry' in process.stdout and not ext.exists()
  elif name=='unwind':assert 'before-intentional-unwind' in process.stdout and 'intentional fixture failure stays a failure' in process.stderr and not ext.exists()
  elif name=='boundary':assert 'bytes=8192 truncated=false' in process.stdout
  elif name=='oversize':assert 'bytes=8192 truncated=true' in process.stdout and len(process.stdout.encode())<8500
  else:assert 'unavailable=' in process.stdout and 'DO_NOT_FOLLOW_PRIVATE_SENTINEL' not in process.stdout
  row['observation_checks_passed']=True;(r/'probe-runs.json').write_text(json.dumps(rows,indent=2)+'\n')
print(json.dumps(dict(status='observation-negative-passed',cases=len(rows),watchdog_negative_before_exit=rows[0]['exit'],watchdog_negative_after_exit=rows[1]['exit'],note='Both intentional watchdog cases remain failed; only parent phase retention changes. Exact extracted watchdog/phase Rust code; not a full crate/product pipe test.'),indent=2))

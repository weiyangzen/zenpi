# Newly authored ZS1-096 offline auditor; never invokes product/history code.
from pathlib import Path
import json,hashlib,re,subprocess,tempfile,sys,traceback,difflib
PACKAGE=Path(sys.argv[1]).resolve()
assert hashlib.sha256((PACKAGE/'manifest.json').read_bytes()).hexdigest()=='4b3fc05994c41164db23839fbf1a8fc7c802db26b801c034d890ec8d281e9d48'
RESULTS=[]
def record(label,passed):RESULTS.append({'label':label,'passed':bool(passed)})
def identity(path):
 data=path.read_bytes();return {'bytes':len(data),'lines':len(data.splitlines()),'sha256':hashlib.sha256(data).hexdigest()}
def inventory(root):return {str(p.relative_to(root)):identity(p) for p in sorted(root.rglob('*')) if p.is_file() and not p.is_symlink()}
def document(name):return json.loads((PACKAGE/name).read_text())
def audit():
 manifest_data=document('manifest.json')
 initial=inventory(PACKAGE);manifest_identity=initial.pop('manifest.json')
 record('manifest exact payload',initial==manifest_data['files'])
 record('readonly no symlinks',all(not p.is_symlink() and not(p.stat().st_mode&0o222) for p in [PACKAGE]+list(PACKAGE.rglob('*'))))
 capture=document('capture.json');summary=document('analysis-summary.json');subject='capture/zenpi/src/render.rs';source_path=PACKAGE/subject;source=source_path.read_bytes();lines=source.splitlines(keepends=True)
 record('formal60098B1706L',identity(source_path)=={'bytes':60098,'lines':1706,'sha256':'b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d'})
 for row in capture['files']:record('capture '+row['path'],identity(PACKAGE/row['path'])=={k:row[k] for k in ['bytes','lines','sha256']})
 baseline=document('baseline-comparison.json');base_path=PACKAGE/baseline['baseline_path'];base=base_path.read_bytes()
 record('baseline30217B901Ldistinct',identity(base_path)==baseline['baseline']=={'bytes':30217,'lines':901,'sha256':'6eaaa712745659ae48f429217c5eb8961dd12e71789f8a2bc70867ee71ba25a7'} and baseline['current']==identity(source_path) and baseline['net_bytes']==29881 and baseline['net_lines']==805 and not baseline['baseline_new_full_read'])
 exact_diff=''.join(difflib.unified_diff(base.decode().splitlines(True),source.decode().splitlines(True),fromfile='baseline/src/render.rs',tofile='current/src/render.rs'))
 record('baseline exact delta',exact_diff==(PACKAGE/'baseline-to-current.diff').read_text() and identity(PACKAGE/'baseline-to-current.diff')==baseline['diff'])
 reads=document('read-binding.json');formal=[r for r in reads if r['source']==subject]
 record('seven contiguous formal reads',len(formal)==7 and formal[0]['byte_range'][0]==0 and formal[-1]['byte_range'][1]==len(source) and all(a['byte_range'][1]==b['byte_range'][0] for a,b in zip(formal,formal[1:])))
 record('35readrecords',len(reads)==35 and summary['all_read_records']==35)
 for row in reads:
  path=PACKAGE/row['source'];data=path.read_bytes();source_lines=data.splitlines(keepends=True);lo,hi=row['lines'];start=sum(map(len,source_lines[:lo-1]));end=sum(map(len,source_lines[:hi]));chunk=(PACKAGE/row['path']).read_bytes()
  record('read '+row['path'],1<=lo<=hi<=len(source_lines) and len(data)==row['source_bytes'] and hashlib.sha256(data).hexdigest()==row['source_sha256'] and row['byte_range']==[start,end] and chunk==data[start:end] and len(chunk)==row['bytes']<=256*1024 and hashlib.sha256(chunk).hexdigest()==row['sha256'])
 units=document('semantic-units.json');maps=document('operation-maps.json');map_ids={r['id'] for r in maps}
 record('61continuousunits',len(units)==61 and units[0]['byte_range'][0]==0 and units[-1]['byte_range'][1]==len(source) and all(a['byte_range'][1]==b['byte_range'][0] for a,b in zip(units,units[1:])))
 record('18substantivemaps',len(map_ids)==18 and all(all(r.get(k) for k in ['source','target','owners','criterion']) for r in maps))
 for row in units:
  lo,hi=row['lines'];chunk=b''.join(lines[lo-1:hi]);start=sum(map(len,lines[:lo-1]));record('unit '+row['id'],row['byte_range']==[start,start+len(chunk)] and len(chunk)==row['bytes'] and hashlib.sha256(chunk).hexdigest()==row['sha256'] and row['map'] in map_ids)
 fn_rows=document('function-bindings.json');found=[]
 for n,line in enumerate(lines,1):
  fn_match=re.match(rb'^\s*(?:pub\s+)?fn\s+(\w+)',line)
  if fn_match is not None:found.append((n,fn_match.group(1).decode()))
 record('67exactfn',len(found)==67 and found==[(r['lines'][0],r['name']) for r in fn_rows])
 record('42production2helpers23tests',all(sum(r['kind']==kind for r in fn_rows)==count for kind,count in [('production',42),('test-helper',2),('source-test',23)]))
 for row in fn_rows:
  lo,hi=row['lines'];chunk=b''.join(lines[lo-1:hi]);opening=lines[lo-1];indent=opening[:len(opening)-len(opening.lstrip())];actual_end=next(n for n in range(lo+1,len(lines)+1) if lines[n-1].rstrip()==indent+b'}')
  record('fn '+str(lo)+' '+row['name'],hi==actual_end and len(chunk)==row['bytes'] and hashlib.sha256(chunk).hexdigest()==row['sha256'] and any(u['id']==row['unit'] and u['lines'][0]<=lo<=u['lines'][1] and u['map']==row['map'] for u in units))
 tests=document('source-tests.json');record('23tests77assert0snapshot0runs',len(tests)==23 and sum(r['ordinary_assert_sites'] for r in tests)==77 and sum(r['snapshot_sites'] for r in tests)==0 and all(not r['executed'] for r in tests))
 for row in tests:
  lo,hi=row['lines'];chunk=b''.join(lines[lo-1:hi]);record('test '+row['id'],hashlib.sha256(chunk).hexdigest()==row['sha256'] and len(re.findall(rb'\bassert(?:_eq|_ne)?!',chunk))==row['ordinary_assert_sites'])
 for row in document('external-preservation-before.json'):record('complete history '+row['path'],inventory(PACKAGE/row['path'])==row['files'])
 inherited=(PACKAGE/'history/inherited-worker-report.md').read_bytes();old_report=(PACKAGE/'history/target096-review320-ready/files/Docs/learn/stage1_pi_mono/targets/zenpi/files/src/render.rs_learn.md').read_bytes()
 record('old reports exact and prefix',inherited==old_report and inherited.startswith((PACKAGE/'history/old-working-report.md').read_bytes()) and baseline['inherited_report_equals_old_final'] and baseline['old_working_report_is_exact_prefix'])
 for row in document('reference-reuse.json'):
  old=PACKAGE/row['package'];record('reference reuse '+row['item'],identity(old/'manifest.json')==row['manifest'] and identity(PACKAGE/row['source_path'])==row['source'] and (old/'evidence'/row['source_path']).read_bytes()==(PACKAGE/row['source_path']).read_bytes() and not row['formal096_credit'] and row['new_runtime_executions']==0)
 preserve=document('preservation-after.json');record('104ready30471regular145tracked484history unchanged',preserve['old_ready_count']==104 and preserve['old_regular_count']==30471 and preserve['tracked_count']==145 and preserve['external_regular_count']==484 and not preserve['differences'])
 record('formal source unchanged at freeze',document('freeze-origin-identities.json')['formal_source_unchanged'])
 record('no runtime execution',summary['runtime_executions']==0)
 report_name=capture['report'];report=PACKAGE/'files'/report_name
 record('unique owned report creation',set(inventory(PACKAGE/'files'))=={report_name} and manifest_data['owned_paths']==[report_name] and identity(report)==summary['report'] and not capture['report_original_exists'])
 record('exact bounded patches',identity(PACKAGE/'report.patch')==summary['patch'] and identity(PACKAGE/'rollback.patch')==summary['rollback'] and (PACKAGE/'report.patch').stat().st_size<=256*1024)
 with tempfile.TemporaryDirectory(prefix='zs1096-offline-') as temp_dir:
  scratch=Path(temp_dir);sentinel=scratch/'unrelated.txt';sentinel.write_bytes(b'preserved\n')
  for patch_name in ['report.patch','rollback.patch']:
   result=subprocess.run(['git','apply','--no-index','--whitespace=nowarn',str(PACKAGE/patch_name)],cwd=scratch,capture_output=True,text=True)
   record('temporary apply '+patch_name,result.returncode==0)
   if result.returncode:RESULTS.append({'label':'patch failure raw','passed':False,'stdout':result.stdout,'stderr':result.stderr})
   if patch_name=='report.patch':record('forward exact report',(scratch/report_name).is_file() and (scratch/report_name).read_bytes()==report.read_bytes())
   else:record('rollback only report removed',not(scratch/report_name).exists() and inventory(scratch)=={'unrelated.txt':identity(sentinel)})
   record('sentinel '+patch_name,sentinel.read_bytes()==b'preserved\n')
 record('post-audit package exact',inventory(PACKAGE)=={**manifest_data['files'],'manifest.json':manifest_identity})
try:audit()
except Exception as error:RESULTS.append({'label':'auditor exception','passed':False,'error':str(error),'traceback':traceback.format_exc()})
for row in RESULTS:print(json.dumps(row,ensure_ascii=False))
failed=sum(not row['passed'] for row in RESULTS)
print(json.dumps({'checks':len(RESULTS),'failed':failed,'structural_only':True,'runtime_execution':False}))
sys.exit(1 if failed else 0)

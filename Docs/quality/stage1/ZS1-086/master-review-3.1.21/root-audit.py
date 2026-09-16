# New ZS1-086 structural auditor. Frozen execution permitted once, outside output only.
from pathlib import Path
import json,hashlib,re,difflib,tempfile,subprocess,sys,traceback
PACKAGE=Path(sys.argv[1]).resolve()
assert hashlib.sha256((PACKAGE/'manifest.json').read_bytes()).hexdigest()=='2c0cf1de5493ddb4ca342d389cce247fed3c64a91913cb8ca23abe1b1c9a1b8f'
RESULTS=[]
def record(label,passed):RESULTS.append({'label':label,'passed':bool(passed)})
def identity(path):
 data=path.read_bytes();return {'bytes':len(data),'lines':len(data.splitlines()),'sha256':hashlib.sha256(data).hexdigest()}
def inventory(root):return {str(p.relative_to(root)):identity(p) for p in sorted(root.rglob('*')) if p.is_file() and not p.is_symlink()}
def document(name):return json.loads((PACKAGE/name).read_text())
def audit():
 manifest_data=document('manifest.json');initial=inventory(PACKAGE);manifest_identity=initial.pop('manifest.json')
 record('exact manifest payload',initial==manifest_data['files'])
 record('readonly no symlinks',all(not p.is_symlink() and not(p.stat().st_mode&0o222) for p in [PACKAGE]+list(PACKAGE.rglob('*'))))
 capture=document('capture.json');summary=document('analysis-summary.json');subject='capture/zenpi/src/view_model.rs';source_path=PACKAGE/subject;source=source_path.read_bytes();lines=source.splitlines(keepends=True)
 record('formal42009B1255L',identity(source_path)=={'bytes':42009,'lines':1255,'sha256':'094833f051ab66d30ce3f6cc74fcc0334ed938e79f5fd8e9269341b62bed08fe'})
 for row in capture['files']:record('capture '+row['path'],identity(PACKAGE/row['path'])=={k:row[k] for k in ['bytes','lines','sha256']})
 baseline=document('baseline-comparison.json');base_path=PACKAGE/baseline['baseline_path'];base=base_path.read_bytes()
 record('baseline40591B1220L',identity(base_path)==baseline['baseline']=={'bytes':40591,'lines':1220,'sha256':'1089a9f352b5aaebc755b51ece8189c1daaca1aa82060d2f009630b915b3abee'} and baseline['current']==identity(source_path) and baseline['net_bytes']==1418 and baseline['net_lines']==35 and not baseline['baseline_new_full_read'])
 exact_diff=''.join(difflib.unified_diff(base.decode().splitlines(True),source.decode().splitlines(True),fromfile='baseline/src/view_model.rs',tofile='current/src/view_model.rs'))
 record('exact baseline diff',exact_diff==(PACKAGE/'baseline-to-current.diff').read_text() and identity(PACKAGE/'baseline-to-current.diff')==baseline['diff'])
 record('old086 current identical',base_path.with_name('current-view_model.rs').read_bytes()==source and baseline['old086_current_exact'])
 reads=document('read-binding.json');formal=[r for r in reads if r['source']==subject]
 record('five contiguous formal reads',len(formal)==5 and formal[0]['byte_range'][0]==0 and formal[-1]['byte_range'][1]==len(source) and all(a['byte_range'][1]==b['byte_range'][0] for a,b in zip(formal,formal[1:])))
 record('29 read records',len(reads)==summary['all_read_records']==29)
 for row in reads:
  data=(PACKAGE/row['source']).read_bytes();source_lines=data.splitlines(keepends=True);lo,hi=row['lines'];start=sum(map(len,source_lines[:lo-1]));end=sum(map(len,source_lines[:hi]));chunk=(PACKAGE/row['path']).read_bytes()
  record('read '+row['path'],1<=lo<=hi<=len(source_lines) and len(data)==row['source_bytes'] and hashlib.sha256(data).hexdigest()==row['source_sha256'] and row['byte_range']==[start,end] and chunk==data[start:end] and len(chunk)==row['bytes']<=256*1024 and hashlib.sha256(chunk).hexdigest()==row['sha256'])
 units=document('semantic-units.json');maps=document('operation-maps.json');map_ids={r['id'] for r in maps}
 record('21 continuous units',len(units)==summary['semantic_units']==21 and units[0]['byte_range'][0]==0 and units[-1]['byte_range'][1]==len(source) and all(a['byte_range'][1]==b['byte_range'][0] for a,b in zip(units,units[1:])))
 record('19 maps with criteria',len(map_ids)==summary['operation_maps']==19 and all(all(r.get(k) for k in ['source','target','owners','criterion']) and r['new_runtime_executions']==0 for r in maps))
 for row in units:
  lo,hi=row['lines'];chunk=b''.join(lines[lo-1:hi]);start=sum(map(len,lines[:lo-1]));record('unit '+row['id'],row['byte_range']==[start,start+len(chunk)] and len(chunk)==row['bytes'] and hashlib.sha256(chunk).hexdigest()==row['sha256'] and row['map'] in map_ids)
 fn_rows=document('function-bindings.json');found=[]
 for n,line in enumerate(lines,1):
  fn_match=re.match(rb'^\s*(?:pub\s+)?(?:const\s+)?fn\s+(\w+)',line)
  if fn_match is not None:found.append((n,fn_match.group(1).decode()))
 record('44 exact functions including const kind',len(found)==summary['functions']==44 and found==[(r['lines'][0],r['name']) for r in fn_rows] and (374,'kind') in found)
 for row in fn_rows:
  lo,hi=row['lines'];chunk=b''.join(lines[lo-1:hi]);opening=lines[lo-1];indent=opening[:len(opening)-len(opening.lstrip())];actual_end=next(n for n in range(lo+1,len(lines)+1) if lines[n-1].rstrip()==indent+b'}')
  record('fn '+str(lo)+' '+row['name'],hi==actual_end and len(chunk)==row['bytes'] and hashlib.sha256(chunk).hexdigest()==row['sha256'] and row['kind']=='production' and any(u['id']==row['unit'] and u['lines'][0]<=lo<=u['lines'][1] and u['map']==row['map'] for u in units))
 record('zero source tests',not re.search(rb'^\s*#\[test\]',source,re.M) and document('source-tests.json')==[] and summary['inline_tests']==0)
 variants=[]
 for line in lines[534:622]:
  variant_match=re.match(rb'^    ([A-Z]\w+)(?:\s*\{|,)',line)
  if variant_match is not None:variants.append(variant_match.group(1).decode())
 record('20 exact event variants',len(variants)==20 and variants==summary['event_variants'])
 tests=document('external-tests.json');test_path='capture/zenpi/tests/view_model.rs';test_data=(PACKAGE/test_path).read_bytes();test_lines=test_data.splitlines(keepends=True);test_reads=[r for r in reads if r['source']==test_path]
 record('two full external reads',len(test_reads)==2 and test_reads[0]['byte_range'][0]==0 and test_reads[0]['byte_range'][1]==test_reads[1]['byte_range'][0] and test_reads[1]['byte_range'][1]==len(test_data))
 record('11 external52assert0snapshot0runs',len(tests)==summary['external_tests']==11 and sum(r['ordinary_assert_sites'] for r in tests)==summary['external_assert_sites']==52 and sum(r['snapshot_sites'] for r in tests)==0 and all(not r['executed'] for r in tests))
 for row in tests:
  lo,hi=row['lines'];chunk=b''.join(test_lines[lo-1:hi]);record('test '+row['id'],row['source']==test_path and test_lines[lo-2].strip()==b'#[test]' and test_lines[lo-1].startswith(('fn '+row['name']).encode()) and test_lines[hi-1].strip()==b'}' and hashlib.sha256(chunk).hexdigest()==row['sha256'] and len(chunk)==row['bytes'] and len(re.findall(rb'\bassert(?:_eq|_ne)?!',chunk))==row['ordinary_assert_sites'])
 for row in document('external-preservation-before.json'):record('complete history '+row['path'],inventory(PACKAGE/row['path'])==row['files'])
 old_report='history/target086-review-3.1.20-ready/files/Docs/learn/stage1_pi_mono/targets/zenpi/files/src/view_model.rs_learn.md';old_reads=[r for r in reads if r['source']==old_report]
 record('old086 full report read',len(old_reads)==2 and old_reads[0]['byte_range'][0]==0 and old_reads[0]['byte_range'][1]==old_reads[1]['byte_range'][0] and old_reads[1]['byte_range'][1]==(PACKAGE/old_report).stat().st_size)
 for row in document('reference-reuse.json'):record('samehash reference '+row['item'],identity(PACKAGE/row['package']/'manifest.json')==row['manifest'] and identity(PACKAGE/row['source_path'])==row['source'] and not row['formal086_credit'] and row['new_runtime_executions']==0)
 preserve=document('preservation-after.json');record('105ready31035regular145tracked945history preserved',preserve['old_ready_count']==105 and preserve['old_regular_count']==31035 and preserve['tracked_count']==145 and preserve['external_regular_count']==945 and not preserve['differences'])
 freeze=document('freeze-origin-identities.json');record('formal unchanged at freeze',freeze['formal_source_unchanged'] and not freeze['files'][0]['changed'])
 for row in freeze['files']:
  if row['changed']:record('separate drift '+row['origin'],identity(PACKAGE/row['drift_path'])==row['now'] and not row['new_full_read_credit'])
 drift=document('tui-drift-analysis.json');record('TUI drift outside read contexts',not drift['changed_read_intersections'] and len(drift['changes'])==5)
 record('no runtime execution',summary['runtime_executions']==0)
 report_name=capture['report'];report=PACKAGE/'files'/report_name
 record('unique owned report creation',set(inventory(PACKAGE/'files'))=={report_name} and manifest_data['owned_paths']==[report_name] and identity(report)==summary['report'] and not capture['report_original_exists'])
 record('exact bounded patches',identity(PACKAGE/'report.patch')==summary['patch'] and identity(PACKAGE/'rollback.patch')==summary['rollback'] and (PACKAGE/'report.patch').stat().st_size<=256*1024)
 with tempfile.TemporaryDirectory(prefix='zs1086-offline-') as temp_dir:
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

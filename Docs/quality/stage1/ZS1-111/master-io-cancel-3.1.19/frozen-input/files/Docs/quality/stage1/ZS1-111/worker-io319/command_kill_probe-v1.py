"""Production CLI kill during approved local command, durable uncertain effect boundary."""
import hashlib,importlib.util,json,os,pathlib,shlex,shutil,sys,time,traceback
EV=pathlib.Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('provider_probe',EV.parents[1]/'provider-kill319/kill_probe.py');h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
h.BINARY=EV.parents[4]/'.ops/output111-io319-current/bin/zenpi'
root=EV/('command-kill-'+sys.argv[1]);root.mkdir(exist_ok=False);(root/'user').mkdir();(root/'user/config.toml').write_text("backend='echo'\n")
(root/'effect.py').write_text('''import os,pathlib,time
root=pathlib.Path.cwd()
with (root/'effects.txt').open('a') as f:
 f.write('START\\n');f.flush();os.fsync(f.fileno())
(root/'child-pid.txt').write_text(str(os.getpid()))
os.write(1,b'CHILD_PARTIAL_319\\n'+b'x'*4096)
os.write(2,b'CHILD_STDERR_319\\n')
end=time.monotonic()+15
while not (root/'release-child').exists() and time.monotonic()<end: time.sleep(.01)
(root/'child-done').write_text('finished own bounded fixture\\n')
''')
clients=[]
try:
 old={'type':'user_shell','id':'killed-command','text':'!exec '+shlex.quote(sys.executable)+' effect.py'}
 one=h.Cli(root,'owner-one');clients.append(one);one.send(old,False)
 approval=one.wait(lambda r:h.evt(r)=='approval_request')[1]['event']['approval'];a=one.send({'type':'approve','id':'approve','approval_id':approval['request_id'],'decision':'allow','remember':True})[1];h.require(a['success'],str(a))
 one.wait(lambda r:h.evt(r)=='tool_progress' and 'CHILD_PARTIAL_319' in json.dumps(r));h.require(one.close(True)==-9,'no SIGKILL')
 shutil.copyfile(root/'session.jsonl',root/'after-kill.session.jsonl');shutil.copyfile(root/'session.jsonl.reconnect',root/'after-kill.session.jsonl.reconnect')
 j=h.journal(root);started=[r['event'] for r in j if r.get('event',{}).get('type')=='tool_execution_started'];h.require(len(started)==1,str(started));op=started[0]['operation_id'];call=started[0]['call_id']
 h.require(not any(r.get('event',{}).get('type') in ('tool_execution_finished','tool_output_captured') for r in j),'uncertain command claimed terminal/capture')
 rawdirs=list(root.glob('.zenpi-output-*'));h.require(len(rawdirs)==1,'missing store');rawdir=rawdirs[0];meta=[json.loads(p.read_text()) for p in rawdir.glob('*.json')];h.require(len(meta)==2 and all(not r['complete'] and not r['finalized'] for r in meta),str(meta))
 rawfiles=list(rawdir.glob('*.raw'));h.require(any(b'CHILD_PARTIAL_319' in p.read_bytes() for p in rawfiles),'raw prefix missing')
 (root/'after-kill-output').mkdir()
 for p in rawdir.iterdir():
  if p.is_file():shutil.copyfile(p,root/'after-kill-output'/p.name)
 h.require((root/'effects.txt').read_text()=='START\n','effect repeated')
 two=h.Cli(root,'owner-two');clients.append(two);inspect=two.command('inspect','/recovery inspect');h.save(root/'recovery-inspect.json',inspect);h.require(inspect['success'],str(inspect));data=inspect['data'];h.require(len(data['operations'])==1 and len(data['tools'])==1 and data['operations'][0]['operation_id']==data['tools'][0]['operation_id']==op,str(data))
 h.check_error(two.send(old)[1],'unknown_outcome');h.check_error(two.send({'type':'user_shell','id':'killed-command','text':'!printf conflict'})[1],'request_id_conflict');h.check_error(two.send({'type':'user_shell','id':'blocked','text':'!printf should-not-run'})[1],'operation_recovery')
 before=(root/'owner-one.stdout.jsonl').read_text().splitlines(True);start=len(two.records);r=two.send({'type':'resume','id':'resume','from_sequence':0})[1];h.require(r['success'],str(r));replayed={v['sequence']:raw.decode() for raw,v in two.records[start:] if v.get('type')=='event'};h.require(all(replayed.get(json.loads(raw)['sequence'])==raw for raw in before if json.loads(raw).get('type')=='event'),'event replay identity changed')
 # The process killed before a sealed terminal reference was journaled.
 # Merely finding an orphan raw file cannot grant a public/model read.
 ref=next(r for r in meta if r['stream']=='stdout');sid=j[0]['session_id'];read=two.send({'type':'tool_output','id':'read-orphan','session_id':sid,'output':{'action':'read','artifact_id':ref['artifact_id'],'call_id':call,'offset':0,'length':32}})[1];h.require(not read['success'],str(read));h.save(root/'orphan-read-result.json',read)
 abandon=two.command('abandon',f'/recovery abandon {op} --yes');h.require(abandon['success'] and abandon['data']['pending_count']==0 and abandon['data']['execution_started'] is False,str(abandon));h.require(two.command('abandon-again',f'/recovery abandon {op} --yes')['success'],'abandon not idempotent');h.check_error(two.send(old)[1],'unknown_outcome')
 h.require((root/'effects.txt').read_text()=='START\n','recovery repeated command')
 (root/'release-child').touch();deadline=time.monotonic()+5
 while not (root/'child-done').exists() and time.monotonic()<deadline: time.sleep(.01)
 h.require((root/'child-done').exists(),'owned bounded child did not finish')
 cleanup=two.send({'type':'tool_output','id':'cleanup','session_id':sid,'output':{'action':'cleanup','confirm':True}})[1];h.save(root/'cleanup-result.json',cleanup);h.require(cleanup['success'] and cleanup['data']['removed']==2,str(cleanup));h.require(not list(rawdir.glob('*.raw')),'orphan raw not cleaned')
 two.close();three=h.Cli(root,'owner-three');clients.append(three);h.check_error(three.send(old)[1],'unknown_outcome');inspection=three.command('inspect-final','/recovery inspect');h.require(inspection['success'] and inspection['data']['pending_count']==0,str(inspection));three.close()
 h.require((root/'effects.txt').read_text()=='START\n','effect duplicated');h.save(root/'result.json',{'passed':True,'binary_sha256':h.digest(h.BINARY),'harness_sha256':h.digest(pathlib.Path(__file__)),'provider_fixture_sha256':h.digest(pathlib.Path(h.__file__)),'effect_count':1,'operation_id':op,'call_id':call,'unknown_provider':False,'unknown_tool':True,'orphan_streams':2,'explicit_cleanup_removed':2,'scope':'SIGKILL bypasses cleanup; bounded child released by fixture, not claimed automatically killed or reaped by application'})
 print('production CLI command SIGKILL/recovery PASS')
except Exception:
 h.save(root/'result.json',{'passed':False,'failure':traceback.format_exc(),'binary_sha256':h.digest(h.BINARY),'harness_sha256':h.digest(pathlib.Path(__file__))});raise
finally:
 (root/'release-child').touch()
 for c in clients:
  if c.p.poll() is None:c.close(True)
 deadline=time.monotonic()+5
 while (root/'child-pid.txt').exists() and not (root/'child-done').exists() and time.monotonic()<deadline:time.sleep(.02)
 h.save(root/'artifact-index.json',[{'path':str(p.relative_to(root)),'bytes':p.stat().st_size,'sha256':h.digest(p)} for p in sorted(root.rglob('*')) if p.is_file() and p.name!='artifact-index.json'])

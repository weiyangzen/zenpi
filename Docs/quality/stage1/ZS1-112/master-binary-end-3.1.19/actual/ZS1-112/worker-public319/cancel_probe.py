import importlib.util,json,os,pathlib,shlex,shutil,sys,time,traceback
EV=pathlib.Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('search_probe',EV/'public_probe.py');s=importlib.util.module_from_spec(spec);spec.loader.exec_module(s);h=s.h;h.BINARY=pathlib.Path(sys.argv[1]).resolve()
root=EV/('cancel-'+sys.argv[2]);root.mkdir(exist_ok=False);(root/'user').mkdir();(root/'fixture').mkdir();(root/'fixture/answer.txt').write_text('needle\n');(root/'bin').mkdir()
rg=pathlib.Path(shutil.which('rg')).resolve();wrapper=root/'bin/rg';wrapper.write_text('#!/bin/sh\nprintf "%s\\n" $$ > .rg-leader\n/bin/sleep 30 &\nprintf "%s\\n" "$!" > .rg-descendant\nwait\nexec '+shlex.quote(str(rg))+' "$@"\n');wrapper.chmod(0o700)
server=s.Server(root,'find',{'pattern':'*.txt','path':'fixture'});(root/'user/config.toml').write_text(f"backend='openai'\nmodel='gpt-4.1'\nwire_api='chat_completions'\nbase_url='{server.url}'\n");clients=[];prior_path=os.environ.get('PATH','')
try:
 os.environ['PATH']=str(root/'bin')+os.pathsep+prior_path
 one=h.Cli(root,'owner-one');clients.append(one);request={'type':'prompt','id':'search','text':'find workspace files'};one.send(request,False);turn=one.wait(lambda r:h.evt(r)=='turn_accepted')[1]['turn_id']
 deadline=time.monotonic()+10
 while not (root/'.rg-descendant').exists() and time.monotonic()<deadline:time.sleep(.01)
 h.require((root/'.rg-descendant').exists(),'search child did not start');pids=[int((root/p).read_text()) for p in ('.rg-leader','.rg-descendant')]
 began=time.monotonic();cancel=one.send({'type':'cancel','id':'cancel','target_id':'search'})[1];h.require(cancel['success'],str(cancel));terminal_raw,terminal=one.wait(lambda r:r.get('id')=='search' and 'success' in r);latency=(time.monotonic()-began)*1000;h.require(not terminal['success'] and 'cancel' in json.dumps(terminal).lower(),str(terminal));h.require(latency<2000,f'cancel latency {latency}')
 deadline=time.monotonic()+3
 def exists(pid):
  try:os.kill(pid,0);return True
  except ProcessLookupError:return False
 while any(exists(pid) for pid in pids) and time.monotonic()<deadline:time.sleep(.01)
 h.require(not any(exists(pid) for pid in pids),f'leader/descendant still exists {pids}');h.require(len(server.requests)==1,'cancel caused provider continuation');one.close()
 j=h.journal(root);h.require(any(r.get('turn',{}).get('role')=='tool' and 'cancelled' in r['turn']['content'] for r in j),'durable cancelled tool result missing');h.require(not (root/'.zenpi/search-index').exists(),'partial index exists')
 two=h.Cli(root,'owner-two');clients.append(two);inspect=two.command('inspect','/recovery inspect');h.require(inspect['success'] and inspect['data']['pending_count']==0,str(inspect));replay,_=two.send(request);h.require(replay==terminal_raw,'terminal replay changed');h.require(len(server.requests)==1,'restart resubmitted');two.close()
 h.save(root/'result.json',{'passed':True,'cancel_to_terminal_ms':latency,'leader_pid':pids[0],'descendant_pid':pids[1],'both_kill_zero_esrch':True,'http_requests':1,'binary_sha256':h.digest(h.BINARY),'script_sha256':h.digest(pathlib.Path(__file__)),'scope':'production CLI JSONL cancellation of controlled installed-rg wrapper while child is live; wrapper delegates to actual rg only if released; no claim that 30-second sleeper performs search semantics'})
 print('production search cancel/restart PASS',round(latency,2),'ms')
except Exception:h.save(root/'result.json',{'passed':False,'failure':traceback.format_exc(),'binary_sha256':h.digest(h.BINARY),'script_sha256':h.digest(pathlib.Path(__file__))});raise
finally:
 os.environ['PATH']=prior_path
 for c in clients:
  if c.p.poll() is None:
   try:
    c.send({'type':'cancel','id':'cleanup-cancel','target_id':'search'});c.close()
   except Exception:c.close(True)
 server.close();h.save(root/'artifact-index.json',[{'path':str(p.relative_to(root)),'bytes':p.stat().st_size,'sha256':h.digest(p)} for p in sorted(root.rglob('*')) if p.is_file() and p.name!='artifact-index.json'])

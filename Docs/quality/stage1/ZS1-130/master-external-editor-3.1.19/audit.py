from pathlib import Path
import ast,hashlib,json
w=Path(__file__).resolve().parent;r=w.parents[2]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_text())
old=read(w/'current-inputs.json');changed=[p for p,h in old.items() if sha(r/p)!=h];assert changed==['tools/tui_external_editor_smoke.py'],changed
before=ast.parse((w/'helper-before-lock-fix.py').read_text());after=ast.parse((r/changed[0]).read_text());a={n.name:ast.dump(n) for n in before.body if isinstance(n,ast.FunctionDef)};b={n.name:ast.dump(n) for n in after.body if isinstance(n,ast.FunctionDef)};assert {k for k in a if a[k]!=b[k]}=={'synchronous_host'}
assert sha(w/'bin/after')=='7a374a3b0764c7a4db9b3b53e36dc07d3d8b17b80f5711805a932a2090e60774'
assert sha(r/'target/release/zenpi')==sha(w/'bin/after')
results={}
for p in sorted(w.glob('*.run.json')):
 d=read(p);log=Path(d['output']);assert sha(log)==d['sha256'] and log.stat().st_size==d['bytes'],p
 expected=101 if d['name'] in ['rust-gates','lib-serial'] else 1 if d['name']=='budget' else 0
 assert d['exit_code']==expected,(p,d['exit_code'])
 results[d['name']]=d['exit_code']
assert results.get('budget')==1
full=read(w/'actual-after.json');assert full['status']=='passed'
count=0
for name,row in full['suites'].items():
 p=Path(row['evidence']);assert sha(p)==row['sha256'];d=read(p);assert d['status']=='passed'
 c=d['checks'];count+=sum(v is True for v in c.values())
 if name=='configuration':
  x=c['near_4mib_checkpoint_rejects_editor_atomically'];assert x['checkpoint_bytes']==4177920 and x['editor_bytes']==262144;count+=1
 elif name=='files-cancel-prefetch':assert sum(isinstance(v,str) for v in c.values())==2
assert count==60,count
old_counts={}
for name in ['original','kill-yank','queued','large','burst','project','menu','approval']:
 d=read(w/f'actual-old-{name}.json');assert d['binary_sha256']==sha(w/'bin/after')
 assert d.get('passed') is True or d.get('status')=='passed'
 if 'checks' in d:assert all(v is True for v in d['checks'].values())
 old_counts[name]=len(d.get('checks',d.get('assertions',[])))
assert sum(old_counts.values())==122,old_counts
starts=0
for p in sorted(w.glob('actual-after-*.child.json'))+[w/'actual-sync-locked.child.json']:
 for e in read(p):
  if e.get('event')=='started':
   assert hashlib.sha256(bytes.fromhex(e['seed_hex'])).hexdigest()==e['seed_sha256']
   assert e['pid']>1 and e['pid']==e['pgid']==e['foreground']
   assert all(e['tty']) and e['canonical'] and e['echo'] and e['blocking']
   assert e['directory_mode']==0o700 and e['mode']==0o600
   assert not any(k.startswith(('ZENPI_','OPENAI_')) for k in e['environment_keys']);starts+=1
assert read(w/'actual-before.child.json')==[] and read(w/'actual-before.http.json')==[]
assert read(w/'actual-after-files-cancel-prefetch.http.json')==[]
http=read(w/'actual-after-roundtrip.http.json');assert len(http)==1
actual=next(x for x in reversed(http[0]['input']) if x.get('role')=='user')['content'];assert actual=='EDITOR_EXPLICIT\n/exit\n!touch must-not-run\n界  \n'
sync=read(w/'actual-sync-locked.json');assert sync['status']=='passed' and sync['checks']['public_sync_host_editor_roundtrip_then_explicit_submit_exact'] is True
assert (w/'actual-sync-locked.sync-build/submitted.txt').read_bytes()=='SYNC_EDITED\n界  \n'.encode()
assert read(w/'consumer-dependency-audit.json')['differences']=={}
budget=read(w/'budget.json');assert budget['ok'] is False and [k for k,v in budget['gates'].items() if not v]==['cold_start']
updated={p:sha(r/p) for p in old};(w/'current-inputs-final.json').write_text(json.dumps(updated,indent=2,sort_keys=True)+'\n')
out={'status':'targeted-evidence-audit-passed','whole_item_accepted':False,'whole_rust_gate':'failed','budget_gate':'failed: cold startup 1208.732708 ms > 1000 ms','inputs':len(updated),'editor_checks':count,'editor_started_records':starts,'old_pty_checks':old_counts,'run_exit_codes':results,'binary_sha256':sha(w/'bin/after'),'helper_sha256':sha(r/changed[0]),'sync_original_dependency_scope':'unseeded compatible consumer only; replaced by exact registry-identity consumer replay'}
(w/'audit-result.json').write_text(json.dumps(out,indent=2,sort_keys=True)+'\n');print(json.dumps(out))

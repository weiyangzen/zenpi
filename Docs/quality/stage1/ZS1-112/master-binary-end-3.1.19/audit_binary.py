from pathlib import Path
import hashlib,json
w=Path(__file__).resolve().parent;r=w.parents[2]
def h(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
binary=load(w/'binary-only-cli.json');assert h(Path(binary['path']))==binary['sha256'];inputs=load(w/'current-binary-only-inputs.json');assert all(h(r/p)==v for p,v in inputs.items())
assert 'fn check_depth_boundary' not in (r/'src/search.rs').read_text()
assert 'let binary_end' in (r/'src/search.rs').read_text()
results={}
for p in w.glob('*.run.json'):
 d=load(p);assert h(Path(d['output']))==d['sha256'];expected=101 if d['name'] in ['binary-before','depth-before'] else 1 if d['name']=='cli-before' else 0;assert d['exit_code']==expected,(p,d['exit_code']);results[d['name']]=d['exit_code']
ev=w/'actual/ZS1-112/worker-public319';cases=[]
for root in sorted(ev.iterdir()):
 if not root.is_dir() or not (root/'result.json').exists():continue
 d=load(root/'result.json');before=root.name.endswith('root-before');assert d['passed'] is (not before)
 for row in load(root/'artifact-index.json'):
  p=root/row['path'];assert h(p)==row['sha256'] and p.stat().st_size==row['bytes']
 if not before:assert d['binary_sha256']==binary['sha256']
 else:assert 'results falsely complete' in d['failure']
 for p in root.glob('*.process.json'):
  process=load(p);assert process['pid']>1 and process['HOME_preserved'] and process['CODEX_HOME_preserved'];assert process['binary_sha256']==d['binary_sha256']
 if not before and root.name.startswith('cancel-'):
  assert d['both_kill_zero_esrch'] and d['http_requests']==1 and d['cancel_to_terminal_ms']<2000
 elif not before:
  request=load(root/'http-request-1.json');actual=json.loads([m for m in request['body']['messages'] if m['role']=='tool'][-1]['content']);assert actual==load(root/'public-tool-result.json')
  journal=[json.loads(line) for line in (root/'session.jsonl').read_text().splitlines()];assert any(x.get('turn',{}).get('role')=='tool' and json.loads(x['turn']['content'])==actual for x in journal)
  if d['case']=='binary':assert actual['output']['matches']==[] and actual['output']['context']==[] and 'binary_content' in actual['output']['truncation_reasons'] and actual['output']['truncated']
 cases.append({'name':root.name,'passed':d['passed']})
assert len(cases)==10 and sum(x['passed'] for x in cases)==8,cases
out={'status':'binary-fix-targeted-audit-passed','whole112_accepted':False,'source_sha256':h(r/'src/search.rs'),'binary':binary,'current_input_count':len(inputs),'cases':cases,'commands':results,'depth_probe':'unsafe candidate withdrawn; original depth requirement remains open; added depth regression currently unresolved','candidate65pass_scope':'historical full candidate including subsequently rejected depth probe, not current full-suite claim'}
(w/'binary-audit.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out))

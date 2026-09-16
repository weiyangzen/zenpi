#!/usr/bin/env python3
"""Read-only packet validation; does not launch Zenpi or query system logs."""
import hashlib,json,pathlib,tarfile
root=pathlib.Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
manifest=json.loads((root/'manifest.json').read_text())
for x in manifest['files']:
 b=(root/x['path']).read_bytes();assert len(b)==x['bytes'] and sha(b)==x['sha256'],x['path']
with tarfile.open(root/'evidence.tar.gz') as t:
 data={m.name:t.extractfile(m).read() for m in t.getmembers() if m.isfile()}
 assert len(data)==len([m for m in t.getmembers() if m.isfile()])
 get=lambda p:json.loads(data[p])
 old=get('original-audit/manifest.json')
 assert sha(data['original-audit/manifest.json'])=='d54a256a5f27e06a8809b5fe19a57cb4bd82cca8949ad8171c2ddb01a0f062c1'
 for x in old['files']:assert sha(data['original-audit/'+x['path']])==x['sha256']
 original=get('original-audit/budget.json');assert original['cold_start']['elapsed_ms']['samples']==[1208.732708,35.488291,33.633667,37.030875,35.111416]
 assert original['budgets']['cold_start_max_ms_max']==1000
 rows=get('diagnosis/observations/summary.json')['rounds'];assert len(rows)==5
 for i,r in enumerate(rows):
  prefix=f'diagnosis/observations/round-{i}/';assert get(prefix+'receipt.json')==r
  times=[m['monotonic_ns'] for m in r['marks']];assert times==sorted(times)
  assert r['index']==i and r['returncode']==0 and not r['timeout'] and r['success_frame']
  frames=[json.loads(x) for x in data[prefix+'stdout.raw'].splitlines()];assert len(frames)==1 and frames[0]['command']=='shutdown' and frames[0]['success']
  assert b'maximum resident set size' in data[prefix+'stderr.raw']
  assert prefix+f'fixture-after/session-{i}.jsonl' in data and prefix+f'fixture-after/session-{i}.jsonl.reconnect' in data
 c=get('diagnosis/policy-correlation.json');assert c['same_boot'] and abs(c['scan_event_interval_ms']-436.6136666666667)<1e-6
 assert 'Identifier=zenpi-cdb646c6c40059cd' in data['diagnosis/codesign.stderr'].decode()
 assert b'DTrace requires additional privileges' in data['diagnosis/dtrace-probe.stderr']
print(json.dumps({'payloads_verified':len(manifest['files']),'archived_files':len(data),'new_diagnostic_rounds':len(rows),'old_samples_preserved':True,'old_gate':'FAIL','cause':'unresolved','product_launched_by_verifier':False}))

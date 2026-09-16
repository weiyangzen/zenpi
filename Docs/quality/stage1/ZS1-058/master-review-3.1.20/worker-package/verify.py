#!/usr/bin/env python3
"""Verify immutable evidence and compare titles. No test execution/collection."""
import hashlib,json,pathlib,tarfile,sys
p=pathlib.Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
j=lambda f:json.loads((p/f).read_text())
for x in j('manifest.json')['files']:
 b=(p/x['path']).read_bytes();assert len(b)==x['bytes'] and sha(b)==x['sha256'],x['path']
inv=j('inventory.json')
with tarfile.open(p/'evidence.tar.gz') as t:
 data={m.name:t.extractfile(m).read() for m in t.getmembers() if m.isfile()}
 get=lambda n:json.loads(data[n])
 assert sha(data['old058/manifest.json'])==inv['old_packet_manifest']['sha256']=='1406f358e7fc3c4e3974c9357f6964afb5421ce6ebc5db80a469e5d93f189d4f'
 old=get('old058/manifest.json');assert len(old['files'])==32
 for x in old['files']:
  b=data['old058/'+x['path']];assert len(b)==x['bytes'] and sha(b)==x['sha256'],x['path']
 for x in inv['master014_files']:
  b=data['master014/'+x['name']];assert len(b)==x['bytes'] and sha(b)==x['sha256'],x['name']
 results=get('master014/actual-replay-results.json')
 cases=[c for f in results['testResults'] for c in f['assertionResults']]
 assert len(results['testResults'])==1 and len(cases)==23 and all(c['status']=='passed' for c in cases)
 assert results['numFailedTests']==results['numPendingTests']==results['numTodoTests']==0
 names=lambda cs:{' > '.join(c['ancestorTitles']+[c['title']]) for c in cs}
 historical=get('old058/reuse/source014-ready-results.json');hc=[c for f in historical['testResults'] for c in f['assertionResults']]
 assert names(cases)==names(hc)=={c['name'] for c in get('old058/collection.json')}
 subject=data['old058/source/packages/agent/test/agent-loop.test.ts'].decode().splitlines()
 for c in get('old058/case-audit.json')['cases']:assert c['title'] in subject[c['line']-1]
 assert len(subject)==1610
 assert inv['frozen_file_count']==1 and inv['frozen_subdirectory_count']==0 and len(inv['children'])==6
 assert [c['name'] for c in inv['children'] if c['scope']=='in-scope']==['agent-loop.test.ts']
 assert inv['ledger_lines']['014'].startswith('- [x]') and inv['ledger_lines']['058'].startswith('- [ ]')
 acceptance=get('master014/acceptance.json');assert acceptance['ok'] and 'ZS1-014' in acceptance['master_receipts_checked']
 if '--upstream' in sys.argv:
  u=pathlib.Path('/Users/wangweiyang/GitHub/pi-mono')
  for s in inv['source_closure_verified']:assert sha((u/s['path']).read_bytes())==s['sha256']
  actual=sorted(x.name for x in (u/'packages/agent/test').iterdir());assert actual==sorted(x['name'] for x in inv['children'])
  for c in inv['children']:
   if c['kind']=='file':assert sha((u/'packages/agent/test'/c['name']).read_bytes())==c['sha256']
print(json.dumps({'old058_payloads':32,'master014_receipt_files':len(inv['master014_files']),'matching_actual_historical_collected_cases':23,'direct_children':6,'frozen_files':1,'frozen_subdirectories':0,'new_tests':0,'new_collections':0,'directory_acceptance':'pending master review','upstream_checked':'--upstream' in sys.argv}))

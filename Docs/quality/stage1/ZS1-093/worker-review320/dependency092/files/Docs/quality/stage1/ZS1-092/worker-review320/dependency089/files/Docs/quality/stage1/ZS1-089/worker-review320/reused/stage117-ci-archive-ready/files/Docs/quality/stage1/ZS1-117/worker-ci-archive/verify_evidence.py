from pathlib import Path
import base64,hashlib,json
ev=Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
baseline=(ev/'baseline-ci.yml').read_bytes();candidate=(ev/'candidate-ci.yml').read_bytes()
assert sha(baseline)=='5d7173fa20a0de949e53525018dad07718a9e4a9d24a5fcd137b056af31792d6'
assert sha(candidate)=='b9e70ba448d157341e0069fcd9ad19eae8f0412aaf0b9e03bbd97b4f2ef59dbd'
old=baseline.decode().splitlines(True);assert candidate.startswith(''.join(old[:-1]).encode())
assert b'! tar -tzf dist/*.tar.gz |' not in candidate
j=json.loads((ev/'three-case-results.json').read_text())
assert [r['runs']['baseline']['exit_code'] for r in j['cases']]==[0,1,0]
assert [r['runs']['candidate']['exit_code'] for r in j['cases']]==[0,1,1]
for row in j['cases']:
 b=base64.b64decode(row['archive_base64']);assert len(b)==row['archive_bytes'] and sha(b)==row['archive_sha256']
 for run in row['runs'].values():assert 'probe.tar.gz: OK' in run['stdout'] and not run['temporary_files_after_exit']
for name,digest in j['extracted_script_sha256'].items():assert sha((ev/(name+'-gate.sh')).read_bytes())==digest
for p in ev.glob('*.json'):
 r=json.loads(p.read_text())
 if isinstance(r,dict) and all(k in r for k in ['exit_code','output','bytes','sha256']):
  b=p.with_suffix('.log').read_bytes();assert len(b)==r['bytes'] and sha(b)==r['sha256'],p
for stem in ['three-case-replay','candidate-shell-syntax']:assert json.loads((ev/(stem+'.json')).read_text())['exit_code']==0
old=json.loads((ev/'old089-preservation.json').read_text());m=Path(old['manifest']);assert sha(m.read_bytes())==old['sha256']
for row in json.loads(m.read_text())['files']:assert sha((m.parent/'files'/row['path']).read_bytes())==row['sha256']
print('117 exact shell delta: three actual old/new archive cases, matching checksums, explicit tar failure and temporary cleanup verified; old089 frozen files unchanged. No hosted Linux/full117 acceptance claim.')

"""Offline byte/receipt/text checks only; never runs Cargo or product code."""
from pathlib import Path
import hashlib,json,re,difflib
E=Path(__file__).resolve().parent
def sha(b):return hashlib.sha256(b).hexdigest()
def read(p):return json.loads((E/p).read_text())
b=(E/'source/baseline-Cargo.toml').read_bytes();c=(E/'source/current-Cargo.toml').read_bytes()
assert (len(b),len(b.splitlines()),sha(b))==(1001,42,'94c25405859cbec9d1a02e7c1a0a4e0dbbfd96e5c6bc476c23b9dc8be7229a65')
assert (len(c),len(c.splitlines()),sha(c))==(1228,50,'d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884')
a=read('context/authority-selector.json');assert a['requirement_digest']=='8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645'
assert a['snapshot_sha256']==sha((E/'context/blueprint-3.1.20.md').read_bytes())
assert a['baseline_files']['ZS1-097']['sha256']==sha(b)
assert (E/'source/baseline-to-current.diff').read_text()==''.join(difflib.unified_diff(b.decode().splitlines(True),c.decode().splitlines(True),fromfile='baseline/Cargo.toml',tofile='current/Cargo.toml'))
inputs=read('inputs.json')['files']
for r in inputs:
 raw=(E/r['snapshot']).read_bytes();assert len(raw)==r['bytes'] and sha(raw)==r['sha256'];assert r['captured_at']
i=read('declaration-inventory.json');ds=i['dependencies'];assert len(ds)==16 and len(i['current_sections']['[dependencies]'])==14 and len(i['baseline_sections']['[dependencies]'])==11
for r in ds:
 assert c.decode().splitlines()[r['line']-1]==r['declaration']
 block=(E/'context/lock'/str(r['name']+'.txt')).read_text();assert f'version = "{r["locked_version"]}"' in block
assert {r['name'] for r in ds}==set(re.findall(r'^ "([^" ]+)",$',(E/'context/lock/zenpi.txt').read_text(),re.M))
assert not next(r for r in ds if r['name']=='crossterm')['registry_source']
assert not next(r for r in ds if r['name']=='crossterm')['registry_checksum']
assert len(read('capability-matrix.json'))==12 and all(not r['product_executed_this_round'] for r in read('capability-matrix.json'))
contexts=read('context-capture.json');assert len(contexts)==10
lock=next(r for r in contexts if r['path'].endswith('/Cargo.lock'));assert lock['full_sha256']=='7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348' and len(lock['excerpts'])==17
for group in contexts:
 assert group['captured_at'] and group['full_sha256']
 for r in group['excerpts']:assert r in inputs and r['source_lines'][0]<=r['source_lines'][1]
for sub,expected in [('ux131','6748f89eb33382e3e87c3c4f1636a5611ca399ddb749f5491fb2f75b737376c5'),('release',None)]:
 folder=E/'history'/sub;manifest=json.loads((folder/'manifest.json').read_text())
 if expected:assert sha((folder/'manifest.json').read_bytes())==expected
 for p in folder.rglob('*'):
  if not p.is_file() or p.name=='manifest.json':continue
  rel=str(p.relative_to(folder));r=manifest['artifacts'].get(rel)
  assert r is not None,rel
  assert sha(p.read_bytes())==(r['sha256'] if isinstance(r,dict) else r),rel
  if isinstance(r,dict):assert len(p.read_bytes())==r['bytes']
result=read('history/ux131/validation/result.json');assert result['status']=='passed' and not result['linux_executed']
assert len(result['command_bindings'])==3 and len(result['pty_cases'])==10
for r in result['command_bindings']:assert r['exit']==0 and sha((E/'history/ux131/validation'/Path(r['log']).name).read_bytes())==r['sha256']
for r in result['pty_cases']:
 name=(r['variant']+'-' if 'variant' in r else '')+r['case']+'.pty';raw=(E/'history/ux131/validation'/name).read_bytes();assert raw==r['raw'].encode() and r['exit']==0
for case in ['queued','paste','csi','utf8']:
 pair=[r for r in result['pty_cases'] if r['case']==case];assert len(pair)==2 and pair[0]['first_hex']==pair[1]['first_hex'] and pair[0]['after_hex']==pair[1]['after_hex']
budget=read('history/release/budget.json');binary=read('history/release/binary.json');build=read('history/release/build-inputs.json')
assert build['Cargo.toml']['sha256']==sha(c) and build['Cargo.lock']['sha256']==lock['full_sha256']
assert binary['sha256']==budget['cold_start']['binary_sha256']=='1d7b2e61ecc9096ad0f5e6cd3f3c03c6820cafe8371d9201de3497bb6b4e8f58'
assert binary['bytes']==budget['release_binary_bytes']==6033168 and budget['dependencies']['normal_count']==15
assert len(budget['gates'])==8 and all(budget['gates'].values()) and not read('history/release/manifest.json')['complete']
assert budget['cold_start']['elapsed_ms']['max']==751.967292
assert not read('receiving-baselines.json')['master']['exists'] and not read('receiving-baselines.json')['worker']['exists']
print(json.dumps(dict(passed=True,inputs=len(inputs),formal_files=1,baseline_lines=42,current_lines=50,current_normal_declarations=15,context_files=10,lock_blocks=17,capability_groups=12,source_functions=0,source_tests=0,product_behavior_runs=0,cargo_runs=0,pty_runs=0,http_runs=0,method='byte integrity and narrow text/receipt validation only'),indent=2))

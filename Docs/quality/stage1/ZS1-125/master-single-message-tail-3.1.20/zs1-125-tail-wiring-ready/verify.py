"""Offline integrity and exact wiring patch replay in a disposable Git repository."""
from pathlib import Path
import json,hashlib,subprocess,tempfile
R=Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
m=json.loads((R/'manifest.json').read_text());paths=['src/tui.rs','tests/tui_transcript_ux.rs']
assert m['product_paths']==paths and m['complete'] is False
actual={str(p.relative_to(R)) for p in R.rglob('*') if p.is_file() and p!=R/'manifest.json'}
assert actual==set(m['artifacts'])
for rel,rec in m['artifacts'].items():
 p=Path(rel);assert not p.is_absolute() and '..' not in p.parts
 b=(R/p).read_bytes();assert len(b)==rec['bytes'] and sha(b)==rec['sha256'],rel
before={p:(R/'before'/p).read_bytes() for p in paths};after={p:(R/'after'/p).read_bytes() for p in paths}
assert sha(before['src/tui.rs'])=='e0e509e507c7c28e4e4ca24709a088c9cedf167c692cf62c3d9bc0ac3658f336'
assert sha(before['tests/tui_transcript_ux.rs'])=='9a5d9c2784ccb1ba5f43e5092ade33e370a259781c4709c059ab3a071d71c13c'
assert after[paths[1]].startswith(before[paths[1]])
a=before[paths[0]].decode();b=after[paths[0]].decode()
assert a.split("fn transcript_window<'a>(")[0]==b.split("fn transcript_window<'a>(")[0]
assert a.split('fn view_block_text(')[1]==b.split('fn view_block_text(')[1]
d=json.loads((R/'evidence/dependency.json').read_text());dependency=Path(d['ready']);assert sha((dependency/'manifest.json').read_bytes())==d['manifest_sha256']
render=(dependency/'after/src/render.rs').read_bytes();assert sha(render)==d['render_sha256']
frozen=json.loads((R/'evidence/frozen-inputs.json').read_text());final=json.loads((R/'evidence/final-inputs.json').read_text())
assert len(final)==4096 and set(final)==set(frozen['files'])
assert {p for p in final if final[p]!=frozen['files'][p]}==set(paths+['src/render.rs'])
for p in paths:assert final[p]['sha256']==sha(after[p])
for p in (R/'commands').glob('*.json'):
 if p.name.endswith('-inputs.json'):continue
 receipt=json.loads(p.read_text());log=(p.parent/receipt['log']).read_bytes();ib=(p.parent/receipt['inputs_file']).read_bytes()
 assert sha(log)==receipt['log_sha256'] and len(log)==receipt['log_bytes'];assert sha(ib)==receipt['inputs_sha256']
 assert receipt['HOME_preserved'] and receipt['CODEX_HOME_preserved']
 assert receipt['environment_overrides']['CARGO_NET_OFFLINE']=='true'
 if receipt['argv'][0]=='cargo':
  assert '--offline' in receipt['argv'] and '--locked' in receipt['argv'] and receipt['inputs_unchanged']
  inputs=json.loads(ib);assert inputs['src/tui.rs']['sha256']==sha(after['src/tui.rs']);assert inputs['src/render.rs']['sha256']==d['render_sha256']
for name,count in [('cargo-test-final-07',16),('cargo-tui-unit-03',23),('cargo-render-unit-04',23),('cargo-render-integration-05',4)]:
 receipt=json.loads((R/'commands'/f'{name}.json').read_text());assert receipt['exit_code']==0
 assert f'{count} passed; 0 failed' in (R/'commands'/receipt['log']).read_text()
assert json.loads((R/'commands/cargo-clippy-final-08.json').read_text())['exit_code']==0
for name in ['cargo-test-01','cargo-clippy-06']:assert json.loads((R/'commands'/f'{name}.json').read_text())['exit_code']==101
ops=[]
with tempfile.TemporaryDirectory(prefix='zs1-125-wiring-replay-') as raw:
 temp=Path(raw)
 def run(argv):
  p=subprocess.run(argv,cwd=temp,capture_output=True,text=True);assert p.returncode==0,(argv,p.stdout,p.stderr)
  return p.stdout
 run(['git','init','--quiet'])
 for p in paths:
  dest=temp/p;dest.parent.mkdir(exist_ok=True,parents=True);dest.write_bytes(before[p])
 sentinel=temp/'unrelated';sentinel.write_bytes(b'preserve\x00\r\n')
 dep=temp/'src/render.rs';dep.write_bytes(render)
 patch=R/'product.patch';stats=run(['git','apply','--numstat',str(patch)])
 assert {line.split('\t')[2] for line in stats.splitlines()}==set(paths)
 for phase,flags in [('forward_check',['--check']),('forward',[]),('reverse_check',['--reverse','--check']),('reverse',['--reverse'])]:
  run(['git','apply',*flags,str(patch)])
  expected=after if phase in ['forward','reverse_check'] else before
  assert all((temp/p).read_bytes()==expected[p] for p in paths)
  assert sentinel.read_bytes()==b'preserve\x00\r\n' and dep.read_bytes()==render
  ops.append(dict(phase=phase,exit_code=0,exact_bytes=True,sentinel_preserved=True,prerequisite_preserved=True))
print(json.dumps(dict(passed=True,manifest_sha256=sha((R/'manifest.json').read_bytes()),artifacts=len(actual),patch_sha256=sha((R/'product.patch').read_bytes()),source_hashes={p:sha(after[p]) for p in paths},operations=ops,cargo_runs_by_verifier=0,full_context_test_assertions_verified=True,master_accepted=False),indent=2))

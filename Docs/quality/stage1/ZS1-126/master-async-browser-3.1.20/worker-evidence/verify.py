"""Offline evidence and exact one-file forward/reverse verifier (Python 3.9+)."""
from pathlib import Path
import argparse,hashlib,json,subprocess,tempfile,tarfile
P=Path(__file__).resolve().parent
H=lambda b:hashlib.sha256(b).hexdigest()
def check():
 manifest=P/'manifest.json'
 if manifest.exists():
  m=json.loads(manifest.read_text())
  for f in m['files']:
   d=(P/f['path']).read_bytes();assert len(d)==f['bytes'] and H(d)==f['sha256'],f['path']
 before=(P/'before.rs').read_bytes();after=(P/'after.rs').read_bytes()
 assert H(before)=='e0e509e507c7c28e4e4ca24709a088c9cedf167c692cf62c3d9bc0ac3658f336'
 assert H(after)=='79b486ef11cccfe5b4fd017c6682e30f23ffcfb1fb6b191165e8befcb674c05c'
 lines=before.splitlines(keepends=True);at=1
 for r in json.loads((P/'full-read.json').read_text()):
  assert r['first']==at and r['read_complete'];d=b''.join(lines[at-1:r['last']]);assert len(d)==r['bytes']<=262144 and H(d)==r['sha256'];at=r['last']+1
 assert at==len(lines)+1==14462
 al=after.splitlines(keepends=True)
 for r in json.loads((P/'unchanged-spans.json').read_text()):
  d=b''.join(lines[r['before_first']-1:r['before_last']]);other=b''.join(al[r['after_first']-1:r['after_last']]);assert d==other and len(d)==r['bytes'] and H(d)==r['sha256']
 ctx=json.loads((P/'final-context.json').read_text())['files'];old={r['path']:r for r in json.loads((P/'context-capture.json').read_text())['files']}
 with tarfile.open(P/'build-context.tar.gz','r:gz') as t:
  assert len(t.getmembers())==len(ctx)==128
  for r in ctx:
   member=t.getmember(r['path']);assert member.isfile();d=t.extractfile(member).read();assert len(d)==r['bytes'] and H(d)==r['sha256']
   if r['path']=='src/tui.rs':assert d==after
   else:assert r==old[r['path']]
 bindings=json.loads((P/'run-source-bindings.json').read_text())
 for name,source in bindings.items():
  r=json.loads((P/name).read_text());assert r['status']=='finished';assert H((P/source).read_bytes())==r['source_sha256'];d=(P/r['log']).read_bytes();assert H(d)==r['sha256'] and len(d)==r['bytes']
  if 'cargo' in r['argv']:
   assert '--offline' in r['argv'] and '--locked' in r['argv'] and '+stable-aarch64-apple-darwin' in r['argv']
   assert any(x.startswith('CARGO_TARGET_DIR=') and 'target126-session-browser-3.1.20/cargo-target' in x for x in r['argv'])
 assert json.loads((P/'negative-sync-reconnected-actual.run.json').read_text())['exit_code']==101
 for name in ['final-tui-tests','final-clippy','restored-browser-tests','final-format-check']:
  assert json.loads((P/(name+'.run.json')).read_text())['exit_code']==0
 # Wiring proof is paired with behavioral barrier/negative-control evidence.
 text=after.decode();host=text[text.index('pub fn run_async_with_profile('):text.index('pub fn run_interactive<')]
 assert host.index('async_session_browser: true')<host.index('reload_state_from_agent(')
 assert host.index('session_browser_host.route(')<host.index('dispatch_slash_command(')
 assert 'session_browser_host.poll(&mut state)' in host
 assert host.index('guard.leave();')<host.index('drop(session_browser_host);')
 assert not any(call in host for call in ['crate::session::list_sessions(', 'crate::session::search_sessions(', 'session_lifecycle_view(', 'session_search_view_in('])
 records=[]
 with tempfile.TemporaryDirectory(prefix='zenpi-126-apply-') as tmp:
  root=Path(tmp);(root/'src').mkdir();source=root/'src/tui.rs';source.write_bytes(before);sentinel=root/'unrelated-sentinel.bin';sentinel_data=b'UNRELATED\x00\xff\nZS1-126\n';sentinel.write_bytes(sentinel_data)
  def run(args):
   out=subprocess.run(args,cwd=root,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT);records.append({'argv':args,'exit_code':out.returncode,'output':out.stdout});assert out.returncode==0,(args,out.stdout)
  run(['git','init','-q']);run(['git','add','src/tui.rs','unrelated-sentinel.bin'])
  patch=str((P/'candidate.patch').resolve())
  run(['git','apply','--check',patch]);run(['git','apply',patch]);assert source.read_bytes()==after and sentinel.read_bytes()==sentinel_data
  forward=H(source.read_bytes())
  run(['git','apply','--reverse','--check',patch]);run(['git','apply','--reverse',patch]);assert source.read_bytes()==before and sentinel.read_bytes()==sentinel_data
  run(['git','diff','--exit-code']);reverse=H(source.read_bytes())
 return {'status':'passed','manifest_checked':manifest.exists(),'files_in_context':128,'unrelated_context_unchanged':127,'full_read_lines':14461,'forward_sha256':forward,'reverse_sha256':reverse,'sentinel_sha256':H(sentinel_data),'commands':records}
if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--receipt');args=parser.parse_args();result=check()
 if args.receipt:Path(args.receipt).write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps(result,indent=2))

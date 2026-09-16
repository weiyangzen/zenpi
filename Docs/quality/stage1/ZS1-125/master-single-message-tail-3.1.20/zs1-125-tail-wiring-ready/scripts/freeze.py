from pathlib import Path
import subprocess,hashlib,json,datetime,shutil,os
M=Path('/Users/wangweiyang/GitHub/zenpi');W=Path('.ops/zs1-125-tail-wiring-work');C=W/'checkout';C.mkdir(exist_ok=False)
def sha(b):return hashlib.sha256(b).hexdigest()
paths=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=M).decode().split('\0');records={};started=datetime.datetime.now(datetime.timezone.utc).isoformat()
for rel in sorted(set(paths)):
 if not rel or rel.split('/')[0] in ['.ops','target','dist']:continue
 p=M/rel
 if not p.exists():continue
 q=C/rel;q.parent.mkdir(parents=True,exist_ok=True)
 if p.is_symlink():
  target=os.readlink(p);q.symlink_to(target);records[rel]=dict(symlink=target);continue
 if not p.is_file():continue
 b=p.read_bytes();q.write_bytes(b);shutil.copymode(p,q);records[rel]=dict(bytes=len(b),sha256=sha(b))
for rel,r in records.items():
 p=M/rel
 if 'symlink' in r:assert os.readlink(p)==r['symlink']
 else:assert sha(p.read_bytes())==r['sha256'],rel
assert records['src/tui.rs']['sha256']=='e0e509e507c7c28e4e4ca24709a088c9cedf167c692cf62c3d9bc0ac3658f336'
(W/'frozen-inputs.json').write_text(json.dumps(dict(source=str(M),started_at=started,finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),files=records,all_reread_unchanged=True),indent=2)+'\n')
for rel in ['src/tui.rs','src/render.rs','tests/tui_transcript_ux.rs']:
 q=W/'before'/rel;q.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(C/rel,q)
# Explicitly authorized immutable dependency; never modify the old ready.
D=Path('.ops/zs1-125-render-tail-ready');manifest=(D/'manifest.json').read_bytes();assert sha(manifest)=='822b201ac8e8f3f5bb4172599ac42c807addbba69c13362d24c26576d36f9581'
b=(D/'after/src/render.rs').read_bytes();assert sha(b)=='b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d';(C/'src/render.rs').write_bytes(b)
(W/'dependency.json').write_text(json.dumps(dict(ready=str(D.resolve()),manifest_sha256=sha(manifest),render_sha256=sha(b),scope='frozen prerequisite only; not in this product patch'),indent=2)+'\n')
print(json.dumps(dict(checkout=str(C.resolve()),files=len(records),bytes=sum(r.get('bytes',0) for r in records.values()),tui=records['src/tui.rs']),indent=2))

#!/usr/bin/env python3
"""Read-only source014 packet/runtime integrity; does not execute test code."""
import argparse,hashlib,json,os
from pathlib import Path
FROZEN_MANIFEST='dd22f389f9bd98ded4766d34ce22251d78bc72a56de0c745de714064aca740eb'
CACHE='.vite/vitest/da39a3ee5e6b4b0d3255bfef95601890afd80709/results.json'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def tree(root):
 entries=[];excluded=[]
 for p in sorted(root.rglob('*')):
  rel=p.relative_to(root).as_posix()
  if rel==CACHE:excluded.append(rel);continue
  if p.is_symlink():entries.append([rel,'symlink',os.readlink(p)])
  elif p.is_file():entries.append([rel,'file',p.stat().st_size,sha(p)])
 encoded=json.dumps(entries,ensure_ascii=True,separators=(',',':')).encode()
 return dict(sha256=hashlib.sha256(encoded).hexdigest(),regular_files=sum(x[1]=='file' for x in entries),symlinks=sum(x[1]=='symlink' for x in entries),excluded_generated_cache=excluded)
def verify(packet,runtime=None,upstream=None):
 assert sha(packet/'manifest.json')==FROZEN_MANIFEST
 m=json.loads((packet/'manifest.json').read_text())
 for x in m['files']:
  p=packet/x['path'];assert p.is_file() and p.stat().st_size==x['bytes'] and sha(p)==x['sha256'],str(p)
 sources=json.loads((packet/'source-inventory.json').read_text())
 for x in sources:
  assert sha(packet/'source'/x['path'])==x['sha256']
  if upstream:assert sha(upstream/x['path'])==x['sha256'],x['path']
 results=json.loads((packet/'results.json').read_text());cases=json.loads((packet/'case-review.json').read_text());actual=[x for f in results['testResults'] for x in f['assertionResults']]
 assert len(actual)==len(cases)==23 and all(x['status']=='passed' for x in actual)
 assert {x['title'] for x in actual}=={x['title'] for x in cases}
 out=dict(manifest_sha256=FROZEN_MANIFEST,payload_files=len(m['files']),source_files=len(sources),recorded_cases=23,new_tests=0,upstream_checked=bool(upstream))
 if runtime:out['runtime']=tree(runtime)
 return out
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--packet',type=Path,required=True);p.add_argument('--runtime',type=Path);p.add_argument('--upstream',type=Path);p.add_argument('--expected',type=Path);a=p.parse_args();out=verify(a.packet,a.runtime,a.upstream)
 if a.expected and a.runtime:assert out['runtime']==json.loads(a.expected.read_text())['runtime'],'Local runtime differs from supplemental audit identity'
 print(json.dumps(out,indent=2))

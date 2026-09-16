#!/usr/bin/env python3
"""Read-only integrity preflight; does not install dependencies or run tests."""
import argparse, hashlib, json
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--packet',required=True,type=Path);p.add_argument('--runtime',type=Path,help='Optional Darwin arm64 node_modules matching the recorded installation');a=p.parse_args()
root=a.packet.resolve()
def digest(f): return hashlib.sha256(f.read_bytes()).hexdigest()
def verify(base,items):
 for x in items:
  rel=Path(x['path'])
  if rel.is_absolute() or '..' in rel.parts: raise ValueError('Unsafe inventory path')
  f=base/rel
  if not f.is_file() or f.stat().st_size!=x['bytes'] or digest(f)!=x['sha256']: raise ValueError('Mismatch: '+str(f))
 return len(items)
m=json.loads((root/'manifest.json').read_text());nf=verify(root,m['files']);ns=verify(root/'source',json.loads((root/'source-inventory.json').read_text()))
r={'manifest_sha256':digest(root/'manifest.json'),'payload_files_verified':nf,'source_files_verified':ns,'tests_run':0,'dependency_files_verified':None}
if a.runtime:
 items=json.loads((root/'installed-dependency-files.json').read_text())
 # Exactly one recorded generated test-result cache; never blanket-exclude package files.
 cache='.vite/vitest/da39a3ee5e6b4b0d3255bfef95601890afd80709/results.json'
 r['dependency_files_verified']=verify(a.runtime.resolve(),[x for x in items if x['path']!=cache]);r['generated_cache_excluded']=cache
print(json.dumps(r,indent=2))

#!/usr/bin/env python3
"""Replay the single successful combination with two explicitly supplied binaries."""
import argparse,hashlib,json,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--fixture-binary',type=Path,required=True);p.add_argument('--evidence',type=Path,required=True);a=p.parse_args();root=Path(__file__).resolve().parent
for x in json.loads((root/'manifest.json').read_text())['files']:
 f=root/x['path'];assert f.stat().st_size==x['bytes'] and hashlib.sha256(f.read_bytes()).hexdigest()==x['sha256'],str(f)
for f,want in [(a.binary,'24721d51a33955057380e20126cbc569706ac17b82c3ce8c370a059a19674c77'),(a.fixture_binary,'a1f5cd223783143cda5c568c342c549bac707a583166a801b32d2ba3d64733da')]:assert hashlib.sha256(f.read_bytes()).hexdigest()==want,str(f)
assert not a.evidence.exists(),'Evidence destination must be new'
subprocess.run([sys.executable,str(root/'select_cancel_pty.py'),'--binary',str(a.binary.resolve()),'--fixture-binary',str(a.fixture_binary.resolve()),'--evidence',str(a.evidence.resolve())],check=True)
subprocess.run([sys.executable,str(root/'verify_evidence.py'),str(a.evidence.resolve())],check=True)

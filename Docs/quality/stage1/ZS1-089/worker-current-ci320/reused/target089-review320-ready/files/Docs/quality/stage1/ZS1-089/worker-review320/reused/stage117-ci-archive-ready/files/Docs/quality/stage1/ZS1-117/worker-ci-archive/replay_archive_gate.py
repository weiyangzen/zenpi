"""Run the extracted real CI gate against the same three byte-identical archives."""
from pathlib import Path
import base64,gzip,hashlib,io,json,os,subprocess,tarfile,tempfile

ev=Path(__file__).resolve().parent
root=ev.parents[4]
candidate=root/'.github/workflows/ci.yml'
sha=lambda b:hashlib.sha256(b).hexdigest()
def gate(path):
 text=path.read_text()
 block=text.split('      - name: Package and verify the production artifact\n',1)[1]
 body=block.split('        run: |\n',1)[1].splitlines()
 assert all(line.startswith('          ') for line in body)
 commands=[line[10:] for line in body]
 assert commands.pop(0)=='tools/release.sh'
 assert commands[0]=='(cd dist && shasum -a 256 -c *.sha256)'
 return '\n'.join(commands)+'\n'
scripts={name:gate(path) for name,path in [('baseline',ev/'baseline-ci.yml'),('candidate',candidate)]}
for name,body in scripts.items():(ev/(name+'-gate.sh')).write_text(body)
(ev/'candidate-ci.yml').write_bytes(candidate.read_bytes())
rows=[]
for case in ['clean','forbidden-name','checksum-consistent-invalid-archive']:
 if case=='checksum-consistent-invalid-archive':data=b'not a tar archive\n'
 else:
  buffer=io.BytesIO()
  with gzip.GzipFile(fileobj=buffer,mode='wb',mtime=0) as zipped:
   with tarfile.open(fileobj=zipped,mode='w') as archive:
    content=b'non-secret test bytes\n'
    entry=tarfile.TarInfo('zenpi/README.md' if case=='clean' else 'zenpi/.env')
    entry.size=len(content);entry.mode=0o644;entry.mtime=0
    archive.addfile(entry,io.BytesIO(content))
  data=buffer.getvalue()
 with tempfile.TemporaryDirectory(prefix='zenpi-ci-117-',dir=root/'.ops') as tmp:
  work=Path(tmp);dist=work/'dist';dist.mkdir();scratch=work/'temp';scratch.mkdir()
  archive=dist/'probe.tar.gz';archive.write_bytes(data)
  (dist/'probe.tar.gz.sha256').write_text(sha(data)+'  probe.tar.gz\n')
  env=os.environ.copy();env['TMPDIR']=str(scratch)
  row={'case':case,'archive_sha256':sha(data),'archive_bytes':len(data),'archive_base64':base64.b64encode(data).decode(),'HOME_preserved':env.get('HOME')==os.environ.get('HOME'),'CODEX_HOME_preserved':env.get('CODEX_HOME')==os.environ.get('CODEX_HOME'),'runs':{}}
  for name,body in scripts.items():
   script=work/(name+'.sh');script.write_text(body)
   result=subprocess.run(['/bin/bash','-e',str(script)],cwd=work,env=env,capture_output=True,text=True,timeout=10)
   row['runs'][name]={'argv':['/bin/bash','-e',str(script)],'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr,'temporary_files_after_exit':[p.name for p in scratch.iterdir()]}
   assert 'probe.tar.gz: OK' in result.stdout
   assert not list(scratch.iterdir()),'temporary archive inventory leaked'
  listed=subprocess.run(['tar','-tzf',str(archive)],capture_output=True,text=True,timeout=10,env=env)
  row['independent_tar']={'exit_code':listed.returncode,'stdout':listed.stdout,'stderr':listed.stderr}
  rows.append(row);print(json.dumps(row),flush=True)
assert [r['runs']['baseline']['exit_code'] for r in rows]==[0,1,0]
assert [r['runs']['candidate']['exit_code'] for r in rows]==[0,1,1]
assert rows[2]['independent_tar']['exit_code']!=0
assert 'Unable to read production archive' in rows[2]['runs']['candidate']['stderr']
assert 'Forbidden entry in production archive' in rows[1]['runs']['candidate']['stderr']
report={'status':'three actual before/after cases passed','cases':rows,'baseline_ci_sha256':sha((ev/'baseline-ci.yml').read_bytes()),'candidate_ci_sha256':sha(candidate.read_bytes()),'extracted_script_sha256':{name:sha(body.encode()) for name,body in scripts.items()},'platform':'Local macOS arm64, actual bash/tar/grep/shasum/mktemp; no hosted GitHub/Linux claim','release_build':'tools/release.sh deliberately excluded from both excerpts; no release build or full117 acceptance claim','tools':{command:subprocess.run(argv,capture_output=True,text=True,timeout=10).stdout.splitlines()[0] for command,argv in [('bash',['/bin/bash','--version']),('tar',['tar','--version'])]}}
(ev/'three-case-results.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({key:value for key,value in report.items() if key!='cases'},indent=2),flush=True)

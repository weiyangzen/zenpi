from pathlib import Path
import subprocess,json,datetime,time,hashlib,sys
P=Path(__file__).resolve().parent
name=sys.argv[1];argv=sys.argv[2:];assert argv
log=P/(name+'.log');receipt=P/(name+'.run.json');assert not log.exists() and not receipt.exists()
m=dict(argv=argv,cwd=str(P/'build-context'),started_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),HOME_preserved=True,CODEX_HOME_preserved=True,network='Cargo offline; no HTTP/PTY',status='running');receipt.write_text(json.dumps(m,indent=2)+'\n');start=time.monotonic()
with log.open('wb') as f:result=subprocess.run(argv,cwd=P/'build-context',stdout=f,stderr=subprocess.STDOUT)
b=log.read_bytes();m.update(exit_code=result.returncode,elapsed_seconds=time.monotonic()-start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),status='finished',log=log.name,bytes=len(b),sha256=hashlib.sha256(b).hexdigest());receipt.write_text(json.dumps(m,indent=2)+'\n');print(json.dumps(m,indent=2));sys.exit(result.returncode)

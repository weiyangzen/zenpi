import datetime,hashlib,json,subprocess,sys,time
from pathlib import Path
r=Path(__file__).resolve().parent
name=sys.argv[1];argv=sys.argv[2:]
start=time.monotonic();stamp=datetime.datetime.now(datetime.timezone.utc).isoformat()
with (r/(name+'.log')).open('xb') as out:
 result=subprocess.run(argv,stdout=out,stderr=subprocess.STDOUT,timeout=180)
b=(r/(name+'.log')).read_bytes()
(r/(name+'.run.json')).write_text(json.dumps(dict(argv=argv,cwd=str(Path.cwd()),started_at=stamp,elapsed_seconds=time.monotonic()-start,exit_code=result.returncode,log_sha256=hashlib.sha256(b).hexdigest(),log_bytes=len(b)),indent=2)+'\n')
print(json.dumps(dict(name=name,exit_code=result.returncode)))
raise SystemExit(result.returncode)

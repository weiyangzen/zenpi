from pathlib import Path
import subprocess,hashlib,json,time,datetime
root=Path.cwd();out=root/'.ops/stage1_execution/async-session-browser-integration'
argv=['python3','-B',str(out/'probe-evidence/probe.py'),'--mode','run','--binary',str(out/'zenpi'),'--expect-binary-sha256','86810a096d7a2f18c55f1b80eb5a25cd51bbb3b02cc6a2762d08b0eea1e33457','--out',str(out/'actual-pty'),'--steps','list,selection,search-failure,project','--pressure-mib','8']
start=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat()
with (out/'actual-pty.log').open('xb') as log:p=subprocess.run(argv,stdout=log,stderr=subprocess.STDOUT)
data=(out/'actual-pty.log').read_bytes();(out/'actual-pty.run.json').write_text(json.dumps(dict(argv=argv,cwd=str(root),started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=p.returncode,log_sha256=hashlib.sha256(data).hexdigest()),indent=2)+'\n')
print('actual-pty',p.returncode,flush=True)
raise SystemExit(p.returncode)

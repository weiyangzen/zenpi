from pathlib import Path
import subprocess,json,hashlib,time,datetime,shutil
root=Path.cwd();out=root/'.ops/stage1_execution/single-message-tail-integration'; dest=out/'entrypoint-final'
for name in ['tui_project_workspace_smoke.py','tui_user_shell_smoke.py']:shutil.copy2(root/'tools'/name,dest/name)
argv=['python3','tools/tui_transcript_ux_smoke.py','--single-message-only','--binary',str(out/'zenpi'),'--evidence',str(dest/'actual-pty')]
start=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat()
with (dest/'actual.log').open('xb') as log:p=subprocess.run(argv,stdout=log,stderr=subprocess.STDOUT)
log=(dest/'actual.log').read_bytes()
(dest/'actual.run.json').write_text(json.dumps(dict(argv=argv,cwd=str(root),started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=p.returncode,log_sha256=hashlib.sha256(log).hexdigest(),script_sha256=hashlib.sha256((root/'tools/tui_transcript_ux_smoke.py').read_bytes()).hexdigest()),indent=2)+'\n')
print('final blueprint-owned entrypoint',p.returncode,flush=True)
raise SystemExit(p.returncode)

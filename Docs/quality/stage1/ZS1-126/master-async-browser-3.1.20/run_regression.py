from pathlib import Path
import subprocess,hashlib,json,time,datetime,shutil
root=Path.cwd();out=root/'.ops/stage1_execution/async-session-browser-integration';(out/'executed-tools').mkdir()
for name in ['tui_project_workspace_smoke.py','tui_user_shell_smoke.py','tui_transcript_ux_smoke.py']:shutil.copy2(root/'tools'/name,out/'executed-tools'/name)
commands=[('project-plus',['python3','-B',str(out/'executed-tools/tui_project_workspace_smoke.py'),'--binary',str(out/'zenpi'),'--evidence',str(out/'project-plus.json')]),('single-tail',['python3','-B',str(out/'executed-tools/tui_transcript_ux_smoke.py'),'--single-message-only','--binary',str(out/'zenpi'),'--evidence',str(out/'single-tail')])]
for name,argv in commands:
    start=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (out/(name+'.log')).open('xb') as log:p=subprocess.run(argv,stdout=log,stderr=subprocess.STDOUT)
    data=(out/(name+'.log')).read_bytes();(out/(name+'.run.json')).write_text(json.dumps(dict(argv=argv,cwd=str(root),started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=p.returncode,log_sha256=hashlib.sha256(data).hexdigest()),indent=2)+'\n')
    print(name,p.returncode,flush=True)
    if p.returncode:raise SystemExit(p.returncode)

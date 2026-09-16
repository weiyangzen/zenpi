from pathlib import Path
import subprocess,json,hashlib,time,datetime
root=Path.cwd();out=root/'.ops/stage1_execution/single-message-tail-integration'
for name,flags in [('multi-message',['--long-history-only']),('transcript-controls',[])]:
    argv=['python3',str(out/'executed-tools/tui_transcript_ux_smoke.py'),'--binary',str(out/'zenpi'),'--evidence',str(out/(name+'.json'))]+flags
    start=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (out/(name+'.log')).open('xb') as log:p=subprocess.run(argv,stdout=log,stderr=subprocess.STDOUT)
    data=(out/(name+'.log')).read_bytes()
    (out/(name+'.run.json')).write_text(json.dumps(dict(argv=argv,cwd=str(root),started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=p.returncode,log_sha256=hashlib.sha256(data).hexdigest()),indent=2)+'\n')
    print(name,p.returncode,flush=True)
    if p.returncode:raise SystemExit(p.returncode)

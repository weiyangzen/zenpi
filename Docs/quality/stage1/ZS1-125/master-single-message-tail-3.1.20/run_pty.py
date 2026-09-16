from pathlib import Path
import subprocess,hashlib,json,time,datetime
root=Path.cwd();out=root/'.ops/stage1_execution/single-message-tail-integration'
for name,binary in [('before',root/'.ops/stage1_execution/attachment-identity-integration/zenpi'),('after',out/'zenpi')]:
    argv=['python3',str(out/'executed-tools/tui_single_message_tail_smoke.py'),'--binary',str(binary),'--evidence',str(out/(name+'-pty'))]
    started=datetime.datetime.now(datetime.timezone.utc).isoformat();start=time.monotonic()
    with (out/(name+'-pty.log')).open('xb') as log:p=subprocess.run(argv,stdout=log,stderr=subprocess.STDOUT)
    data=(out/(name+'-pty.log')).read_bytes()
    (out/(name+'-pty.run.json')).write_text(json.dumps(dict(argv=argv,cwd=str(root),started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=p.returncode,log_sha256=hashlib.sha256(data).hexdigest(),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()),indent=2)+'\n')
    print(name,p.returncode,flush=True)

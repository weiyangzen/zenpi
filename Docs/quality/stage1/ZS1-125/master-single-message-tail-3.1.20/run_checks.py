from pathlib import Path
import subprocess,hashlib,json,time,datetime,shutil
root=Path.cwd(); out=root/'.ops/stage1_execution/single-message-tail-integration'
commands=[('render-unit',['test','--lib','render::tests::']),('tui-unit',['test','--lib','tui::tests::']),('integration',['test','--test','tui_transcript_ux','--test','render_markdown']),('clippy',['clippy','--lib','--tests','--','-D','warnings']),('format',['fmt','--all','--','--check']),('build',['build','--bin','zenpi'])]
for name,args in commands:
    argv=['cargo','+stable-aarch64-apple-darwin',args[0]]+([] if args[0]=='fmt' else ['--offline','--locked'])+args[1:]
    started=datetime.datetime.now(datetime.timezone.utc).isoformat(); start=time.monotonic()
    with (out/(name+'.log')).open('xb') as f:p=subprocess.run(argv,stdout=f,stderr=subprocess.STDOUT)
    log=(out/(name+'.log')).read_bytes()
    (out/(name+'.run.json')).write_text(json.dumps(dict(argv=argv,cwd=str(root),started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=p.returncode,log_sha256=hashlib.sha256(log).hexdigest()),indent=2)+'\n')
    print(name,p.returncode,flush=True)
    if p.returncode:raise SystemExit(p.returncode)
shutil.copy2(root/'target/debug/zenpi',out/'zenpi');(out/'zenpi').chmod(0o555)
print('binary',hashlib.sha256((out/'zenpi').read_bytes()).hexdigest(),flush=True)

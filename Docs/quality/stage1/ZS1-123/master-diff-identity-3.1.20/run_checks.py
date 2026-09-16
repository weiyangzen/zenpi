from pathlib import Path
import subprocess, hashlib, json, time, datetime, shutil, os
root=Path.cwd(); out=root/'.ops/stage1_execution/diff-identity-integration'
env=os.environ.copy()
env.update(GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null')
commands=[('owner-inline',['test','--lib','slash_actions::','--','--nocapture']),
 ('owner-integration',['test','--test','slash_actions','--test','tui_command_palette','--test','headless_protocol']),
 ('clippy',['clippy','--lib','--test','slash_actions','--test','tui_command_palette','--test','headless_protocol','--','-D','warnings']),
 ('format',['fmt','--all','--','--check']),('build',['build','--bin','zenpi'])]
for name,args in commands:
 argv=['cargo','+stable-aarch64-apple-darwin',args[0]]+([] if args[0]=='fmt' else ['--offline','--locked'])+args[1:]
 started=datetime.datetime.now(datetime.timezone.utc).isoformat(); start=time.monotonic()
 with (out/(name+'.log')).open('xb') as log: p=subprocess.run(argv,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 raw=(out/(name+'.log')).read_bytes()
 (out/(name+'.run.json')).write_text(json.dumps(dict(argv=argv,cwd=str(root),env_diff={'GIT_CONFIG_NOSYSTEM':'1','GIT_CONFIG_GLOBAL':'/dev/null'},started_at=started,elapsed_seconds=time.monotonic()-start,exit_code=p.returncode,log_sha256=hashlib.sha256(raw).hexdigest()),indent=2)+'\n')
 print(name,p.returncode,flush=True)
 if p.returncode: raise SystemExit(p.returncode)
shutil.copy2(root/'target/debug/zenpi',out/'zenpi'); (out/'zenpi').chmod(0o555)
print('binary',hashlib.sha256((out/'zenpi').read_bytes()).hexdigest(),flush=True)

from pathlib import Path
import subprocess,sys
r=Path(__file__).resolve().parents[3];w=Path(__file__).resolve().parent
runner=r/'.ops/stage1_execution/chat-json-strict-integration/run.py'
binary=w/'bin/after'
cases=[('original','tui_composer_smoke.py',[]),('kill-yank','tui_composer_smoke.py',['--kill-yank-only']),('queued','tui_composer_smoke.py',['--queued-shell-paste-only']),('large','tui_composer_smoke.py',['--large-paste-only']),('burst','tui_composer_smoke.py',['--burst-only']),('project','tui_project_workspace_smoke.py',[]),('menu','tui_command_palette_smoke.py',[]),('approval','tui_approval_smoke.py',[])]
for name,script,flags in cases:
 command=[sys.executable,str(runner),str(w),'old-'+name,sys.executable,str(r/'tools'/script),'--binary',str(binary),'--evidence',str(w/('actual-old-'+name+'.json')),*flags]
 subprocess.run(command,cwd=r,check=True)
 print('completed '+name,flush=True)

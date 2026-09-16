from pathlib import Path
import json, subprocess, sys

root=Path.cwd()
p=Path(__file__).resolve().parent
relative=str(p.relative_to(root))
binary=str(p/'zenpi')
cases=[
 ('background', ['tools/tui_approval_smoke.py','--background-only']),
 ('approval', ['tools/tui_approval_smoke.py']),
 ('bentobox', ['tools/stage1_host_smoke.py','--case','bentobox']),
 ('picker', [str(p/'tools/tui_project_workspace_smoke.py')]),
 ('new', [str(p/'tools/tui_session_new_smoke.py')]),
 ('policy', [str(p/'tools/tui_session_new_smoke.py'),'--policy-only']),
 ('recovery', [str(p/'tools/tui_session_new_smoke.py'),'--recovery-only']),
]
results=[]
for name,args in cases:
 argv=[sys.executable,'.ops/stage1_execution/chat-json-strict-integration/run.py',relative,name,
       sys.executable,*args,'--binary',binary,'--evidence',str(p/(name+'.json'))]
 result=subprocess.run(argv,cwd=root)
 results.append(dict(name=name,exit_code=result.returncode))
 (p/'ux-run-summary.json').write_text(json.dumps(results,indent=2)+'\n')
 if result.returncode:
  sys.exit(result.returncode)
print('Seven release UX suites completed on one frozen binary.')

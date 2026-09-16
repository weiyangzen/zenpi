"""Finite real Git / headless / PTY path identity cases. No provider prompts."""
from pathlib import Path
import argparse, hashlib, json, os, subprocess, sys
sys.path.insert(0,str(Path('/Users/wangweiyang/GitHub/zenpi/tools')))
from tui_project_workspace_smoke import Terminal, screen_text

p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
a=p.parse_args();binary=a.binary.resolve();out=a.out.resolve();assert not out.exists();out.mkdir()
root=out/'fixture'; work=root/'initial'; config=root/'fixture'; sessions=root/'sessions'
for d in [work,config,sessions]:d.mkdir(parents=True)
env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))}
env.update(ZENPI_HOME=str(config),TERM='xterm-256color',GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null',GIT_GLOB_PATHSPECS='1',GIT_ICASE_PATHSPECS='1')
(config/'config.toml').write_text('backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:9/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
(config/'auth.json').write_text('{"OPENAI_API_KEY":"local-diff-fixture"}\n');(config/'auth.json').chmod(0o600)
result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':{},'git_commands':[],'cases':[],
 'tui_messages':[],'screens':[],'provider_prompts_sent':0,'tui_processes':1,'headless_processes':1,
 'environment_overrides':{'GIT_GLOB_PATHSPECS':'1','GIT_ICASE_PATHSPECS':'1'},'termios_equality_checked':False}
setup=env.copy()
for key in ['GIT_GLOB_PATHSPECS','GIT_ICASE_PATHSPECS','GIT_NOGLOB_PATHSPECS','GIT_LITERAL_PATHSPECS']:setup.pop(key,None)
def git(args):
 argv=['git','--literal-pathspecs',*args];r=subprocess.run(argv,cwd=work,env=setup,capture_output=True,text=True,timeout=10)
 result['git_commands'].append(dict(argv=argv,exit_code=r.returncode,stdout=r.stdout,stderr=r.stderr));assert r.returncode==0
git(['init','-q']);git(['config','user.name','Local diff fixture']);git(['config','user.email','diff@example.test']);git(['config','commit.gpgsign','false'])
names=[(r'note\report.txt','note/report.txt',True),('selected[ab].txt','selecteda.txt',True),
 (':(glob)*.txt','glob-neighbor.txt',True),('--output=NOT_AN_OPTION.txt','option-neighbor.txt',True),
 ('工作 note.txt','space-neighbor.txt',True),(r'new\only.txt','new/only.txt',False)]
for selected,neighbor,tracked in names:
 for name in [selected,neighbor]:
  path=work/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text('BEFORE\n')
 if tracked:git(['add','--',selected,neighbor])
git(['commit','-qm','fixture files'])
for i,(selected,neighbor,tracked) in enumerate(names):
 expected=f'SELECTED_{i:02}_DIFF_CONTENT';wrong=f'NEIGHBOR_{i:02}_DIFF_CONTENT'
 (work/selected).write_text(expected+'\n');(work/neighbor).write_text(wrong+'\n')
 result['cases'].append(dict(path=selected,neighbor=neighbor,tracked=tracked,expected=expected,wrong=wrong,
  selected_inode=(work/selected).stat().st_ino,neighbor_inode=(work/neighbor).stat().st_ino))
 assert (work/selected).stat().st_ino!=(work/neighbor).stat().st_ino
inputs=[dict(schema_version=2,type='command',id=f'diff-{i}',text='/diff '+json.dumps(case['path'],ensure_ascii=False)) for i,case in enumerate(result['cases'])]
inputs.append(dict(schema_version=2,type='shutdown',id='shutdown'))
(out/'headless-input.jsonl').write_text(''.join(json.dumps(x,ensure_ascii=False)+'\n' for x in inputs))
t=None
try:
 argv=[str(binary),'--mode','headless','--session',str(sessions/'headless.jsonl')]
 proc=subprocess.run(argv,input=(out/'headless-input.jsonl').read_bytes(),cwd=work,env=env,capture_output=True,timeout=30)
 (out/'headless.stdout').write_bytes(proc.stdout);(out/'headless.stderr').write_bytes(proc.stderr)
 result['headless_argv']=argv;result['headless_exit_code']=proc.returncode
 assert proc.returncode==0,proc.stderr
 rows=[json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
 for i,case in enumerate(result['cases']):
  response=next(v for v in rows if v.get('type')=='response' and v.get('id')==f'diff-{i}')
  data=response.get('data',{});diff=data.get('diff','')
  result['checks'][f'headless-{i}-exact-path-and-hunk']=bool(response.get('success') and data.get('path')==case['path'] and '+'+case['expected'] in diff and case['wrong'] not in diff)
 result['checks']['headless-shutdown']=any(v.get('id')=='shutdown' and v.get('success') for v in rows)
 t=Terminal(binary,root,env)
 def active():
  snapshot=json.loads(t.checkpoint.read_text());return next(row for row in snapshot['project_state'] if row['name']==snapshot['projects'][snapshot['active']])
 for i,case in enumerate(result['cases']):
  t.clear_input();before=len(active()['messages'])
  t.command('/diff '+json.dumps(case['path'],ensure_ascii=False))
  t.wait(lambda:len(active()['messages'])>before)
  messages=active()['messages'][before:];text='\n'.join(row['text'] for row in messages)
  good='+'+case['expected'] in text and case['wrong'] not in text and ('diff '+case['path']) in text
  result['checks'][f'tui-{i}-exact-hunk']=good
  if good:t.expect(('+'+case['expected']).encode())
  result['tui_messages'].append(messages);result['screens'].append(screen_text(bytes(t.output)).decode())
  (out/f'tui-{i:02}-checkpoint.json').write_bytes(t.checkpoint.read_bytes())
 result['checks']['no-option-side-effect']=not (work/'NOT_AN_OPTION.txt').exists()
 t.close();result['checks']['tui-exit-and-alternate-screen-restored']=True
except Exception as e:
 result['error']=repr(e)
finally:
 if t:
  (out/'tui.pty.log').write_bytes(t.output);t.cleanup()
 result['passed']='error' not in result and len(result['checks'])==15 and all(result['checks'].values())
 (out/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
 print(json.dumps({'passed':result['passed'],'checks':result['checks'],'error':result.get('error')},ensure_ascii=False))
raise SystemExit(0 if result['passed'] else 1)

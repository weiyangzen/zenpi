"""Finite aggregate quoted-path checks through real headless and TUI owners."""
from pathlib import Path
import argparse,datetime,hashlib,json,os,subprocess,sys,time
sys.path.insert(0,'/Users/wangweiyang/GitHub/zenpi/tools')
from tui_project_workspace_smoke import Terminal,screen_text
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args()
binary=a.binary.resolve();out=a.out.resolve();assert not out.exists();out.mkdir();root=out/'fixture';work=root/'initial';config=root/'config';sessions=root/'sessions'
for d in (work,config,sessions):d.mkdir(parents=True)
env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(config),TERM='xterm-256color',GIT_CONFIG_NOSYSTEM='1',GIT_CONFIG_GLOBAL='/dev/null',GIT_GLOB_PATHSPECS='1',GIT_ICASE_PATHSPECS='1')
(config/'config.toml').write_text('backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:9/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n');(config/'auth.json').write_text('{"OPENAI_API_KEY":"local-quoted-diff-fixture"}\n');(config/'auth.json').chmod(0o600)
setup=env.copy()
for key in ('GIT_GLOB_PATHSPECS','GIT_ICASE_PATHSPECS','GIT_NOGLOB_PATHSPECS','GIT_LITERAL_PATHSPECS'):setup.pop(key,None)
result={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'checks':{},'git_commands':[],'cases':[],'headless':[],'tui_messages':[],'screens':[],'provider_prompts_sent':0,'headless_processes':0,'tui_processes':0,'termios_equality_checked':False,'environment_overrides':{'GIT_CONFIG_NOSYSTEM':'1','GIT_CONFIG_GLOBAL':'/dev/null','GIT_GLOB_PATHSPECS':'1','GIT_ICASE_PATHSPECS':'1','ZENPI_HOME':'fixture/config','TERM':'xterm-256color'}}
def git(args):
 argv=['git','--literal-pathspecs',*args];p=subprocess.run(argv,cwd=work,env=setup,capture_output=True,text=True,timeout=10);result['git_commands'].append({'argv':argv,'exit_code':p.returncode,'stdout':p.stdout,'stderr':p.stderr});assert p.returncode==0,p.stderr

git(['init','-q']);git(['config','user.name','Quoted fixture']);git(['config','user.email','quoted@example.test']);git(['config','commit.gpgsign','false'])
for folder in ['scope[ab]','scope[ab]/note','scopea']:
 (work/folder).mkdir(parents=True,exist_ok=True);(work/folder/'seed').write_text('seed\n')
git(['add','.']);git(['commit','-qm','tracked directory seeds'])
names=[r'note\report.txt','note/report.txt','space file.txt','quote"name.txt','中文 文件.txt']
for i,name in enumerate(names):
 path=work/'scope[ab]'/name;path.write_text(f'QUOTED_REAL_HOST_{i}\n');result['cases'].append({'path':'scope[ab]/'+name,'inode':path.stat().st_ino,'marker':f'QUOTED_REAL_HOST_{i}'})
assert result['cases'][0]['inode']!=result['cases'][1]['inode']
(work/'scopea/neighbor').write_text('OUTSIDE_LITERAL_SCOPE\n')
scopes=[None,'.','scope[ab]']
def command(scope):return '/diff' if scope is None else '/diff '+json.dumps(scope)
def good(text,scope):return all('+'+c['marker'] in text for c in result['cases']) and (scope!='scope[ab]' or 'OUTSIDE_LITERAL_SCOPE' not in text)
t=None
try:
 for quote in ['true','false']:
  git(['config','core.quotePath',quote]);inputs=[dict(schema_version=2,type='command',id=str(i),text=command(scope)) for i,scope in enumerate(scopes)]+[dict(schema_version=2,type='shutdown',id='shutdown')];data=''.join(json.dumps(x)+'\n' for x in inputs).encode();(out/f'headless-{quote}.input.jsonl').write_bytes(data)
  argv=[str(binary),'--mode','headless','--session',str(sessions/f'headless-{quote}.jsonl')];start=time.monotonic();proc=subprocess.run(argv,input=data,cwd=work,env=env,capture_output=True,timeout=30);result['headless_processes']+=1
  (out/f'headless-{quote}.stdout').write_bytes(proc.stdout);(out/f'headless-{quote}.stderr').write_bytes(proc.stderr);result['headless'].append({'argv':argv,'exit_code':proc.returncode,'elapsed_seconds':time.monotonic()-start});assert proc.returncode==0,proc.stderr
  rows=[json.loads(line) for line in proc.stdout.splitlines() if line.strip()]
  for i,scope in enumerate(scopes):
   response=next(v for v in rows if v.get('type')=='response' and v.get('id')==str(i));value=response.get('data',{});result['checks'][f'headless-{quote}-{i}-all-quoted-files']=bool(response.get('success') and value.get('path')==(scope or '.') and good(value.get('diff',''),scope))
  result['checks'][f'headless-{quote}-shutdown']=any(v.get('id')=='shutdown' and v.get('success') for v in rows)
 t=Terminal(binary,root,env);result['tui_processes']=1
 def active():
  snapshot=json.loads(t.checkpoint.read_text());return next(row for row in snapshot['project_state'] if row['name']==snapshot['projects'][snapshot['active']])
 for quote in ['true','false']:
  git(['config','core.quotePath',quote])
  for i,scope in enumerate(scopes):
   t.clear_input();before=len(active()['messages']);t.command(command(scope));t.wait(lambda:len(active()['messages'])>before);messages=active()['messages'][before:];text='\n'.join(row['text'] for row in messages);passed=good(text,scope);result['checks'][f'tui-{quote}-{i}-all-quoted-files']=passed
   if passed:t.expect(b'+QUOTED_REAL_HOST_4')
   result['tui_messages'].append({'quotePath':quote,'scope':scope,'messages':messages});result['screens'].append(screen_text(bytes(t.output)).decode());(out/f'tui-{quote}-{i}.checkpoint.json').write_bytes(t.checkpoint.read_bytes())
 t.close();result['checks']['tui-exit-alternate-screen-restored']=True
except Exception as error:result['error']=repr(error)
finally:
 if t:
  (out/'tui.pty.log').write_bytes(t.output)
  try:t.cleanup()
  except Exception as error:result['cleanup_error']=repr(error)
 result['ended_at']=datetime.datetime.now(datetime.timezone.utc).isoformat();result['passed']='error' not in result and 'cleanup_error' not in result and len(result['checks'])==15 and all(result['checks'].values());(out/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'passed':result['passed'],'checks':result['checks'],'error':result.get('error')},ensure_ascii=False))
raise SystemExit(0 if result['passed'] else 1)

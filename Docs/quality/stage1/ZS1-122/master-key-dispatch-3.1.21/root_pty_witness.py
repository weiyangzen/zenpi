"""Fresh controller PTY witness for locally built before/after binaries. Derived from reviewed worker v3; new outputs and explicit visible menu assertion."""
from pathlib import Path
import codecs,fcntl,hashlib,http.server,json,os,pty,select,struct,subprocess,sys,termios,threading,time,unicodedata
W=Path(__file__).resolve().parent
class Screen:
 def __init__(self):self.rows=[[' ']*200 for _ in range(40)];self.x=0;self.y=0;self.pending='';self.decoder=codecs.getincrementaldecoder('utf-8')('replace')
 def feed(self,data):
  s=self.pending+self.decoder.decode(data);i=0
  while i<len(s):
   c=s[i]
   if c=='\x1b':
    if i+1>=len(s):break
    if s[i+1]=='[':
     j=i+2
     while j<len(s) and not ('@'<=s[j]<='~'):j+=1
     if j==len(s):break
     raw=s[i+2:j];cmd=s[j];args=[int(n) if n.isdigit() else 0 for n in raw.lstrip('?').split(';')];n=args[0] or 1
     if cmd in 'Hf':self.y=max(0,min(39,n-1));self.x=max(0,min(199,(args[1] if len(args)>1 and args[1] else 1)-1))
     elif cmd=='G':self.x=min(199,n-1)
     elif cmd=='d':self.y=min(39,n-1)
     elif cmd=='A':self.y=max(0,self.y-n)
     elif cmd=='B':self.y=min(39,self.y+n)
     elif cmd=='C':self.x=min(199,self.x+n)
     elif cmd=='D':self.x=max(0,self.x-n)
     elif cmd=='J' and args[0] in (2,3):self.rows=[[' ']*200 for _ in range(40)]
     elif cmd=='K':
      lo=0 if args[0] in (1,2) else self.x;hi=200 if args[0] in (0,2) else self.x+1
      self.rows[self.y][lo:hi]=[' ']*(hi-lo)
     i=j+1;continue
    if s[i+1]==']':
     j=i+2
     while j<len(s) and s[j]!='\x07' and s[j:j+2]!='\x1b\\':j+=1
     if j==len(s):break
     i=j+(1 if s[j]=='\x07' else 2);continue
    i+=2;continue
   if c=='\r':self.x=0
   elif c=='\n':self.y=min(39,self.y+1)
   elif c=='\b':self.x=max(0,self.x-1)
   elif ord(c)>=32:
    width=0 if unicodedata.combining(c) else (2 if unicodedata.east_asian_width(c) in 'WF' else 1)
    if self.x<200 and width:
     self.rows[self.y][self.x]=c
     if width==2 and self.x+1<200:self.rows[self.y][self.x+1]=''
     self.x=min(199,self.x+width)
   i+=1
  self.pending=s[i:]
 def text(self):return '\n'.join(''.join(r).rstrip() for r in self.rows)
def run(label):
 out=W/('root-pty-'+label);out.mkdir();cfg=out/'config';cfg.mkdir();cwd=out/'workspace';cwd.mkdir();(out/'sessions').mkdir();requests=[]
 class Handler(http.server.BaseHTTPRequestHandler):
  def log_message(self,*args):pass
  def do_POST(self):requests.append({'method':'POST','path':self.path});self.send_error(503)
  def do_GET(self):requests.append({'method':'GET','path':self.path});self.send_error(503)
 server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
 (cfg/'config.toml').write_text('backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:%d/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n'%server.server_port)
 (cfg/'auth.json').write_text(json.dumps({'OPENAI_API_KEY':'local-inert-fixture'}));(cfg/'auth.json').chmod(0o600)
 marker=out/'unexpected-editor-invocation';editor=out/'editor.sh';editor.write_text('#!/bin/sh\nprintf invoked > "'+str(marker)+'"\n');editor.chmod(0o700)
 env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_'))};env.update(ZENPI_HOME=str(cfg),TERM='xterm-256color',VISUAL=str(editor),EDITOR=str(editor))
 master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',40,200,0,0))
 def setup():os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
 binary=W/'binaries'/(label+'-zenpi');argv=[str(binary),'--tui','--session',str(out/'sessions/initial.jsonl')]
 proc=subprocess.Popen(argv,cwd=cwd,env=env,stdin=slave,stdout=slave,stderr=slave,preexec_fn=setup);screen=Screen();raw=bytearray();events=[];checks={};start=time.monotonic()
 def drain(seconds=.25):
  until=time.monotonic()+seconds
  while time.monotonic()<until:
   if select.select([master],[],[],min(.05,max(0,until-time.monotonic())))[0]:
    try:b=os.read(master,65536)
    except OSError:return
    if not b:return
    raw.extend(b);screen.feed(b)
 def wait(predicate):
  until=time.monotonic()+8
  while time.monotonic()<until:
   drain(.05)
   if predicate():return
   if proc.poll() is not None:break
  raise AssertionError('PTY condition timeout: '+screen.text())
 def send(name,data):events.append({'name':name,'hex':data.hex(),'at_seconds':time.monotonic()-start});os.write(master,data);drain(.3)
 def snap(name): (out/(name+'.txt')).write_text(screen.text()+'\n')
 def draft():
  try:
   v=json.loads((cfg/'project-tabs.json').read_text());active=v['projects'][v['active']];return next(r['draft']['input'] for r in v['project_state'] if r['name']==active)
  except (FileNotFoundError,ValueError,KeyError,StopIteration):return None
 error=None
 try:
  wait(lambda:'Ctrl-G editor' in screen.text());snap('01-idle');checks['idle_hint']=True
  send('type slash',b'/');send('type m',b'm');send('type o',b'o');wait(lambda:draft()=='/mo' and 'command palette active' in screen.text());drain(.3);snap('02-choices');checks['actual_command_palette_visible']=True
  checks['menu_hint_present']='Ctrl-G editor' in screen.text();assert checks['menu_hint_present']==(label=='before')
  send('blocked Ctrl-G',b'\x07');snap('03-blocked-editor');assert draft()=='/mo' and not marker.exists();checks['menu_ctrl_g_preserves_draft_and_does_not_start_editor']=True
  send('dismiss choices',b'\x1b');drain(.3);wait(lambda:'Ctrl-G editor' in screen.text());snap('04-dismissed');assert draft()=='/mo';checks['dismissed_hint_restored']=True
  send('clear draft',b'\x15');wait(lambda:draft()=='')
  send('ordinary Unicode','界é🙂'.encode());wait(lambda:draft()=='界é🙂');snap('05-unicode');checks['unicode_exact']=True
  send('Ctrl-U',b'\x15');wait(lambda:draft()=='')
  send('Ctrl-Y',b'\x19');wait(lambda:draft()=='界é🙂');snap('06-yanked');checks['kill_yank_exact']=True
  send('clear before exit',b'\x15');wait(lambda:draft()=='');send('Ctrl-D exit',b'\x04');proc.wait(timeout=5);drain(.1);checks['clean_exit']=proc.returncode==0;checks['terminal_cleanup_sequences_observed']=b'\x1b[?1049l' in raw and b'\x1b[?25h' in raw;checks['provider_requests_zero']=len(requests)==0
  assert checks['clean_exit'] and checks['terminal_cleanup_sequences_observed'] and checks['provider_requests_zero']
 except BaseException as exc:error=repr(exc)
 finally:
  if proc.poll() is None:
   proc.terminate()
   try:proc.wait(timeout=2)
   except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=2)
  (out/'output.bin').write_bytes(raw);(out/'events.json').write_text(json.dumps(events,indent=2));(out/'requests.json').write_text(json.dumps(requests));snap('last-screen');server.shutdown();server.server_close();os.close(master);os.close(slave)
  result={'label':label,'argv':argv,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'checks':checks,'error':error,'exit_code':proc.returncode,'runtime_seconds':time.monotonic()-start,'scope':'Actual PTY bytes, wide footer, menu Ctrl-G, Escape, Unicode, Ctrl-U/Y/D. Super/Repeat/Release coverage is synthetic production Event dispatch only. Terminal cleanup bytes are observed; post-exit termios restore is not asserted on macOS (v1 ENOTTY).'};(out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
 if error:raise AssertionError(error)
if __name__=='__main__':run(sys.argv[1])

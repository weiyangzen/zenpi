"""Fresh controller PTY run derived from the reviewed worker probe; unique fixture/output and current controller binary."""
from pathlib import Path
import fcntl, hashlib, json, os, pty, re, select, signal, struct, termios, time, unicodedata
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

W=Path(__file__).resolve().parent
B=W/'binaries/after-zenpi'
OUT=W/'root-pty-current'
OUT.mkdir()  # Deliberately refuse a second output/run.
ROOT=OUT/'fixture';PROJECT=ROOT/'project';CONFIG=ROOT/'config';SESSIONS=ROOT/'sessions'
for p in [PROJECT,CONFIG,SESSIONS]:p.mkdir(parents=True)
for i in range(200):(PROJECT/f'candidate-{i:03}.txt').write_text('fixture\n')
requests=[]
class RejectRequests(BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def do_GET(self):
        requests.append({'method':self.command,'path':self.path})
        self.send_response(503);self.end_headers()
    do_POST=do_GET
server=HTTPServer(('127.0.0.1',0),RejectRequests)
(CONFIG/'config.toml').write_text(f'backend="openai"\nmodel="gpt-4.1"\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nwire_api="responses"\nrequires_openai_auth=false\nmax_retries=0\n')
(CONFIG/'auth.json').write_text('{"OPENAI_API_KEY":"private-completion-fixture"}\n')
(CONFIG/'auth.json').chmod(0o600)
env={k:v for k,v in os.environ.items() if not k.startswith(('ZENPI_','OPENAI_','ANTHROPIC_','GOOGLE_','GEMINI_','AZURE_'))}
env.update(ZENPI_HOME=str(CONFIG),TERM='xterm-256color')
argv=[str(B),'--mode','tui','--backend','openai','--session',str(SESSIONS/'initial.jsonl')]
result={'binary_sha256':hashlib.sha256(B.read_bytes()).hexdigest(),'argv':argv,'cwd':str(PROJECT),
        'started_unix':time.time(),'checks':{},'screens':{},'actions':[]}
raw=bytearray();pid=None;fd=None

def screen():
    # Cursor/erase text projection only; styling is not asserted. The raw PTY is retained.
    grid=[[' ']*140 for _ in range(40)];x=y=0
    text=raw.decode('utf8',errors='replace');i=0
    while i<len(text):
        c=text[i]
        if c=='\x1b':
            m=re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])',text[i:])
            if m:
                args,_,op=m.groups();i+=len(m.group());v=[int(n) if n.isdigit() else 0 for n in args.lstrip('?').split(';')];n=v[0] or 1
                if op in 'Hf':y=min(39,max(0,n-1));x=min(139,max(0,(v[1] if len(v)>1 and v[1] else 1)-1))
                elif op=='G':x=min(139,max(0,n-1))
                elif op=='d':y=min(39,max(0,n-1))
                elif op=='A':y=max(0,y-n)
                elif op=='B':y=min(39,y+n)
                elif op=='C':x=min(139,x+n)
                elif op=='D':x=max(0,x-n)
                elif op=='J' and v[0] in [2,3]:grid=[[' ']*140 for _ in range(40)]
                elif op=='K':
                    a,b=(0,140) if v[0]==2 else ((0,x+1) if v[0]==1 else (x,140));grid[y][a:b]=[' ']*(b-a)
                elif op=='h' and args=='?1049':grid=[[' ']*140 for _ in range(40)];x=y=0
                continue
            m=re.match(r'\x1b\].*?(?:\a|\x1b\\)',text[i:],re.S)
            i+=len(m.group()) if m else 2;continue
        if c=='\r':x=0
        elif c=='\n':y=min(39,y+1)
        elif c=='\b':x=max(0,x-1)
        elif ord(c)>=32:
            cells=0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in ('W','F') else 1
            if not cells and x: grid[y][x-1]+=c
            elif cells and x<140:
                grid[y][x]=c
                if cells==2 and x+1<140:grid[y][x+1]=''
                x+=cells
        i+=1
    return '\n'.join(''.join(row) for row in grid)

def drain(seconds=.05):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        if select.select([fd],[],[],max(0,end-time.monotonic()))[0]:
            try:b=os.read(fd,65536)
            except OSError:return
            if not b:return
            raw.extend(b)
def wait(predicate,timeout=12):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        drain()
        try:
            if predicate():return
        except (FileNotFoundError,json.JSONDecodeError):pass
    raise AssertionError('Condition timed out.\n'+screen())
def send(b):
    result['actions'].append({'time':time.monotonic(),'hex':b.hex()})
    os.write(fd,b)
def type_text(text):
    for c in text:send(c.encode());drain(.035)
def draft():
    p=json.loads((CONFIG/'project-tabs.json').read_text());active=p['projects'][p['active']]
    return next(row['draft']['input'] for row in p['project_state'] if row['name']==active)
def check(name,value):
    result['checks'][name]=bool(value)
    assert value,name
def capture(name):
    shown=screen();(OUT/(name+'.screen.txt')).write_text(shown+'\n');result['screens'][name]=name+'.screen.txt';return shown

try:
    pid,fd=pty.fork()
    if pid==0:
        os.chdir(PROJECT);os.execve(str(B),argv,env)
    fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',40,140,0,0))
    threading.Thread(target=server.serve_forever,daemon=True).start()
    wait(lambda:'Prompt' in screen())
    type_text('/attach candidate-')
    wait(lambda:'Partial results' in screen() and 'display limit' in screen())
    shown=capture('partial')
    check('partial_notice_and_guidance_visible','Narrow the path or filename prefix' in shown)
    wait(lambda:draft()=='/attach candidate-')
    check('partial_scan_keeps_draft',draft()=='/attach candidate-')
    type_text('199')
    wait(lambda:'candidate-199.txt' in screen() and 'Partial results' not in screen())
    capture('narrowed');check('narrower_prefix_replaces_partial_status',True)
    send(b'\t');wait(lambda:draft()=='/attach "candidate-199.txt" ')
    check('tab_completes_actual_file_without_submission',True)
    send(b'\x15');wait(lambda:draft()=='')
    type_text('/attach no-match-prefix')
    wait(lambda:'No matching files in this directory' in screen())
    capture('complete-empty');check('empty_complete_is_directory_scoped',True)
    send(b'\x1b');wait(lambda:'No matching files in this directory' not in screen())
    check('escape_preserves_empty_query_draft',draft()=='/attach no-match-prefix')
    capture('dismissed')
    send(b'\x15');wait(lambda:draft()=='')
    type_text('/quit');drain(.2);send(b'\r')
    end=time.monotonic()+8
    while time.monotonic()<end:
        drain()
        done,status=os.waitpid(pid,os.WNOHANG)
        if done:
            pid=None;result['wait_status']=status
            check('normal_exit',os.WIFEXITED(status) and os.WEXITSTATUS(status)==0)
            break
    check('process_reaped',pid is None)
    check('terminal_alternate_screen_restored',b'\x1b[?1049l' in raw)
    check('zero_model_requests',len(requests)==0)
    records=[json.loads(line) for line in (SESSIONS/'initial.jsonl').read_text().splitlines()]
    check('no_provider_turn_submitted',not any(r.get('kind')=='turn' for r in records))
except Exception as error:
    result['error']=repr(error)
finally:
    if fd is not None:os.close(fd)
    if pid is not None:
        try:os.kill(pid,signal.SIGKILL)
        except ProcessLookupError:pass
        os.waitpid(pid,0)
    server.shutdown();server.server_close()
    (OUT/'terminal.raw').write_bytes(raw)
    result['http_requests']=requests;result['finished_unix']=time.time()
    result['ok']='error' not in result and all(result['checks'].values())
    (OUT/'result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'ok':result['ok'],'checks':result['checks'],'error':result.get('error')}))
raise SystemExit(0 if result['ok'] else 1)

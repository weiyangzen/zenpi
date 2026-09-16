"""One fresh real-PTY approval lifecycle observation. No archived runner imports."""
import codecs, fcntl, hashlib, http.server, json, os, pathlib, pty, re, select, signal, struct, subprocess, sys, termios, threading, time, unicodedata
root=pathlib.Path(__file__).resolve().parent
out=root/'root-pty-evidence';out.mkdir(exist_ok=False)
workspace=out/'workspace';workspace.mkdir();config=out/'config';config.mkdir()
checks=[];requests=[];released=threading.Event();entered=threading.Event()
def check(name, ok, **details):
    checks.append(dict(name=name,ok=bool(ok),**details))
    (out/'checks-progress.json').write_text(json.dumps(checks,indent=2)+'\n')
    if not ok: raise AssertionError(name)
class HTTP(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_GET(self):
        requests.append(dict(method='GET',path=self.path))
        if self.path=='/gate':
            entered.set()
            released.wait(20)
            body=b'TIMER-GATE-COMPLETED\n';self.send_response(200);self.end_headers();self.wfile.write(body)
        else:self.send_error(503)
    def do_POST(self):
        requests.append(dict(method='POST',path=self.path));self.send_error(503)
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),HTTP)
port=server.server_port
class Screen:
    def __init__(self):
        self.rows=[[' ']*180 for _ in range(40)];self.x=0;self.y=0;self.pending='';self.saved=(0,0)
    def feed(self,data):
        self.pending+=data
        while self.pending:
            text=self.pending
            if text[0]=='\x1b':
                if len(text)<2:return
                if text[1]=='[':
                    m=re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])',text)
                    if not m:return
                    self.pending=text[m.end():];raw,_,cmd=m.groups()
                    vals=[int(v or 0) for v in raw.lstrip('?').split(';')] if raw.lstrip('?') else [0]
                    n=vals[0] or 1
                    if cmd in 'Hf':self.y=max(0,min(39,n-1));self.x=max(0,min(179,(vals[1] if len(vals)>1 and vals[1] else 1)-1))
                    elif cmd=='A':self.y=max(0,self.y-n)
                    elif cmd=='B':self.y=min(39,self.y+n)
                    elif cmd=='C':self.x=min(179,self.x+n)
                    elif cmd=='D':self.x=max(0,self.x-n)
                    elif cmd=='G':self.x=max(0,min(179,n-1))
                    elif cmd=='d':self.y=max(0,min(39,n-1))
                    elif cmd=='J':
                        if vals[0] in (2,3):self.rows=[[' ']*180 for _ in range(40)]
                        elif vals[0]==0:
                            self.rows[self.y][self.x:]=[' ']*(180-self.x)
                            for y in range(self.y+1,40):self.rows[y]=[' ']*180
                    elif cmd=='K':
                        a,b=(0,180) if vals[0]==2 else ((0,self.x+1) if vals[0]==1 else (self.x,180))
                        self.rows[self.y][a:b]=[' ']*(b-a)
                    elif cmd=='s':self.saved=(self.x,self.y)
                    elif cmd=='u':self.x,self.y=self.saved
                    continue
                if text[1]==']':
                    m=re.search(r'\x07|\x1b\\',text[2:])
                    if not m:return
                    self.pending=text[2+m.end():];continue
                if text[1] in '()':
                    if len(text)<3:return
                    self.pending=text[3:];continue
                self.pending=text[2:];continue
            self.pending=text[1:];c=text[0]
            if c=='\r':self.x=0
            elif c=='\n':self.y=min(39,self.y+1)
            elif c=='\b':self.x=max(0,self.x-1)
            elif ord(c)>=32:
                if unicodedata.combining(c):continue
                self.rows[self.y][min(self.x,179)]=c
                w=2 if unicodedata.east_asian_width(c) in 'WF' else 1
                if w==2 and self.x<179:self.rows[self.y][self.x+1]=''
                self.x=min(179,self.x+w)
    def text(self):return '\n'.join(''.join(row) for row in self.rows)
    def seconds(self):
        m=re.search(r'\((\d+)s · Ctrl-C\)',self.text())
        return int(m.group(1)) if m else None
screen=Screen();decode=codecs.getincrementaldecoder('utf-8')('replace');raw=bytearray()
master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',40,180,0,0))
binpath=root/'root-bin'/'zenpi'
env={k:os.environ[k] for k in ('HOME','PATH','LANG','TMPDIR') if k in os.environ}
env.update(TERM='xterm-256color',ZENPI_HOME=str(config),ZENPI_BASE_URL=f'http://127.0.0.1:{port}/v1',ZENPI_API_KEY='local-fixture-only',ZENPI_MODEL='gpt-4.1',ZENPI_BACKEND='openai')
def child_session():
    os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
p=subprocess.Popen([str(binpath),'--tui','--session',str(workspace/'session.jsonl')],cwd=workspace,env=env,stdin=slave,stdout=slave,stderr=slave,preexec_fn=child_session,close_fds=True)
os.close(slave);threading.Thread(target=server.serve_forever,daemon=True).start()
def pump(duration):
    end=time.monotonic()+duration
    while time.monotonic()<end:
        if select.select([master],[],[],min(.05,max(0,end-time.monotonic())))[0]:
            try:b=os.read(master,65536)
            except OSError:return
            if not b:return
            raw.extend(b);screen.feed(decode.decode(b));(out/'terminal.raw').write_bytes(raw);(out/'latest-screen.txt').write_text(screen.text())
def until(predicate,timeout=8):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        pump(.08)
        if predicate():return True
        if p.poll() is not None:return False
    return False
def send(b):os.write(master,b)
def snap(name):
    (out/(name+'.txt')).write_text(screen.text())
    return screen.seconds()
error=None
try:
    check('real TUI ready',until(lambda:'Prompt' in screen.text()))
    send(b'\x1b[200~'+f'!curl --max-time 20 --silent --show-error http://127.0.0.1:{port}/gate'.encode()+b'\x1b[201~');pump(.25);send(b'\r')
    check('real shell approval shown',until(lambda:'Approval required' in screen.text() and 'user_shell' in screen.text()))
    send(b'\x1b');pump(.2)
    send(b'\x1bc') # Alt-C: no assistant answer, changes status without a provider call.
    check('non-approval status reached while waiting',until(lambda:'No assistant answer to copy' in screen.text()))
    a=snap('pending-start');pump(1.3);b=snap('pending-end')
    check('pending clock holds after hidden modal and unrelated status',a is not None and a==b,start=a,end=b)
    check('shell side effect has not started before approval',not entered.is_set())
    send(b'\x1ba');pump(.2);send(b'y');pump(.15);send(b'\r')
    check('approved shell reached independent HTTP gate',until(entered.is_set))
    a=snap('running-start')
    check('clock resumes while approved shell still waits at gate',until(lambda:screen.seconds() is not None and a is not None and screen.seconds()>a,4),start=a,end=screen.seconds())
    snap('running-end');released.set()
    check('host terminal completion clears busy header',until(lambda:'TIMER-GATE-COMPLETED' in screen.text() and screen.seconds() is None))
    snap('completed')
    send(b'/quit');pump(.25);send(b'\r');check('clean TUI exit',until(lambda:p.poll() is not None,8))
    check('process exit success',p.returncode==0,exit=p.returncode)
except Exception as exc:error=repr(exc)
finally:
    released.set()
    if p.poll() is None:
        send(b'\x03');pump(.2);send(b'/quit\r');pump(.5)
        if p.poll() is None:
            try:p.terminate();p.wait(3)
            except (OSError,subprocess.TimeoutExpired) as cleanup_error:
                error=repr(cleanup_error) if error is None else error+'; cleanup '+repr(cleanup_error)
    pump(.1);os.close(master)
    if p.poll() is None:
        try:p.wait(3)
        except subprocess.TimeoutExpired:pass
    server.shutdown();server.server_close()
    (out/'terminal.raw').write_bytes(raw)
    (out/'final-screen.txt').write_text(screen.text())
    turns=[];event_counts={}
    journal=workspace/'session.jsonl'
    if journal.exists():
        for line in journal.read_text().splitlines():
            try:entry=json.loads(line)
            except ValueError:continue
            event=entry.get('event',entry)
            kind=event.get('type','unknown');event_counts[kind]=event_counts.get(kind,0)+1
            if event.get('type') in ('turn_started','turn_completed'):turns.append(entry)
    checks.append(dict(name='no model HTTP requests',ok=all(r['method']=='GET' and r['path']=='/gate' for r in requests),requests=requests))
    checks.append(dict(name='alternate screen restored',ok=b'\x1b[?1049l' in raw))
    checks.append(dict(name='one local shell journal turn',ok=event_counts.get('turn',0)==1,event_counts=event_counts))
    result=dict(checks=checks,error=error,exit=p.returncode,actual_pty=True,pid=p.pid,model_requests=sum(not(r['method']=='GET' and r['path']=='/gate') for r in requests),gate_requests=sum(r['path']=='/gate' for r in requests),journal_turn_markers=len(turns),journal_event_counts=event_counts,binary=dict(bytes=binpath.stat().st_size,sha256=hashlib.sha256(binpath.read_bytes()).hexdigest()))
    (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    sys.exit(0 if error is None and all(c['ok'] for c in checks) else 1)

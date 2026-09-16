from pathlib import Path
import hashlib,json,shutil,subprocess,datetime
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/dir050-current-3.1.21';D.mkdir(exist_ok=False)
W=Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-050-directory321-ready')
def info(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
assert info(W/'manifest.json')['sha256']=='3777f82051efec3e2b33d0e188326cfd3a5e2c3e6bf4aa41c8f6817315df3587'
shutil.copytree(W,D/'worker');argv=['python3','-B',str(D/'worker/verify_offline.py'),str(D/'worker')];start=datetime.datetime.now(datetime.timezone.utc).isoformat()
p=subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(D/'offline.stdout.json').write_bytes(p.stdout);(D/'offline.stderr.log').write_bytes(p.stderr)
(D/'offline.run.json').write_text(json.dumps(dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,stdout=info(D/'offline.stdout.json'),stderr=info(D/'offline.stderr.log')),indent=2)+'\n')
assert p.returncode==0,p.stderr
x=json.loads(p.stdout);assert x['passed'] and len(x['checks'])==25
print('Independent directory050 offline verifier:25/25; no runtime execution.')

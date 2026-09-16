from pathlib import Path
import os, sys, json, subprocess, datetime, hashlib, time
W = Path(__file__).resolve().parent
C = W / 'checkout'
label, argv = sys.argv[1], sys.argv[2:]
log, receipt = W / (label + '.log'), W / (label + '.json')
assert not log.exists() and not receipt.exists()
def sha(b): return hashlib.sha256(b).hexdigest()
def inventory():
    return {str(p.relative_to(C)): ({'symlink': os.readlink(p)} if p.is_symlink() else {'bytes': p.stat().st_size, 'sha256': sha(p.read_bytes())}) for p in sorted(C.rglob('*')) if p.is_file() or p.is_symlink()}
before = inventory()
(W / (label + '-inputs.json')).write_text(json.dumps(before, indent=2) + '\n')
env = dict(os.environ)
env['CARGO_TARGET_DIR'] = str(W / 'target')
env['CARGO_NET_OFFLINE'] = 'true'
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
stamp = time.monotonic()
with log.open('wb') as f:
    p = subprocess.run(argv, cwd=C, stdout=f, stderr=subprocess.STDOUT, env=env)
after = inventory()
b = log.read_bytes()
r = dict(argv=argv, cwd=str(C), started_at=start, ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), elapsed_seconds=time.monotonic()-stamp, exit_code=p.returncode, environment_overrides={k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_NET_OFFLINE']}, HOME_preserved=True, CODEX_HOME_preserved=True, inputs_file=label+'-inputs.json', inputs_sha256=sha((W/(label+'-inputs.json')).read_bytes()), inputs_unchanged=before==after, changed_inputs={k:{'before':before.get(k),'after':after.get(k)} for k in sorted(before.keys()|after.keys()) if before.get(k)!=after.get(k)}, log=log.name, log_bytes=len(b), log_sha256=sha(b))
receipt.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps(r, indent=2))
print(b.decode(errors='replace')[-6000:])
sys.exit(p.returncode)

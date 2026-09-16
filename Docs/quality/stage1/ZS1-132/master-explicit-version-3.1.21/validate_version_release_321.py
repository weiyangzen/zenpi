from pathlib import Path
import datetime, hashlib, json, os, shutil, signal, subprocess

R = Path(__file__).resolve().parents[2]
D = R / '.ops/stage1_execution/version-release-3.1.21'
D.mkdir(exist_ok=False)
V = R / '.ops/stage1_execution/blueprint-version-current-3.1.21'

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

def run(name, argv, allowed=(0,), timeout=300):
    env = os.environ.copy()
    env.update(CARGO_NET_OFFLINE='true', CARGO_BUILD_JOBS='2', PYTHONDONTWRITEBYTECODE='1')
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    timed_out = False
    with (D / (name + '.log')).open('xb') as log:
        child = subprocess.Popen(argv, cwd=R, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=10)
            code = 124
    dump(D / (name + '.run.json'), dict(argv=argv, cwd=str(R), started_at=started,
         ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=code,
         pid=child.pid, reaped=child.poll() is not None, timed_out=timed_out,
         HOME_preserved=env.get('HOME') == os.environ.get('HOME'),
         CODEX_HOME_preserved=env.get('CODEX_HOME') == os.environ.get('CODEX_HOME'), log=meta(D / (name + '.log'))))
    print(name, code, flush=True)
    assert code in allowed, name
    return code

previous = json.loads((V / 'inputs-after.json').read_text())
inputs = {p: meta(R / p) for p in previous}
drift = {p: dict(previous=previous[p], current=inputs[p]) for p in inputs if inputs[p] != previous[p]}
assert set(drift) <= {'tools/generate_stage1_gantt.py'}, drift
dump(D / 'input-delta-since-integration.json', drift)
dump(D / 'inputs.json', inputs)
assert inputs['src/headless.rs']['sha256'] == '521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0'
for name in ['root-tests', 'root-clippy', 'root-fmt']:
    assert json.loads((V / (name + '.run.json')).read_text())['exit_code'] == 0
shutil.copy2(V / 'worker/evidence/probe.py', D / 'probe.py')
# The unchanged benchmark builds production and performs its first launches.
# Fixed three samples and original gates; no prewarming or retry on failure.
code = run('budget', ['python3', 'tools/bench_runtime.py', '--samples', '3', '--output', str(D / 'budget.json'), '--startup-evidence-dir', str(D / 'startup')], allowed=(0, 1))
budget = json.loads((D / 'budget.json').read_text())
assert code == (0 if budget['ok'] else 1)
binary = D / 'zenpi-release'
shutil.copy2(R / 'target/release/zenpi', binary)
binary.chmod(0o555)
dump(D / 'binary.json', meta(binary))
assert budget['cold_start']['binary_sha256'] == meta(binary)['sha256']
old = R / '.ops/stage1_execution/shutdown-current-3.1.21/release-hosts/zenpi-release'
assert meta(old)['sha256'] == '6891df5d13a373eff656f8c99a5b5367f9ddb4e502f159d0d925162ec6bccbc4'
run('probe-before', ['python3', str(D / 'probe.py'), '--binary', str(old), '--out', str(D / 'before')], allowed=(1,), timeout=120)
run('probe-after', ['python3', str(D / 'probe.py'), '--binary', str(binary), '--out', str(D / 'after')], timeout=120)
before = json.loads((D / 'before/result.json').read_text())
after = json.loads((D / 'after/result.json').read_text())
assert len(before['checks']) == len(after['checks']) == 55
assert sum(not v for v in before['checks'].values()) == 5
assert all(after['checks'].values()) and after['exit_code'] == before['exit_code'] == 0
assert all(meta(R / p) == value for p, value in inputs.items()), 'captured input drift'
dump(D / 'master-verification.json', dict(scope='explicit blueprint version selection; not full Stage1 acceptance',
     current_binary=meta(binary), current_headless=inputs['src/headless.rs'],
     budget_ok=budget['ok'], gates=budget['gates'], startup_samples_ms=budget['cold_start']['elapsed_ms']['samples'],
     before_checks=55, before_failed=5, after_checks=55, after_failed=0, input_count=len(inputs), input_stable=True,
     complete=False))
print('Current production version selection verified; full Stage1 remains incomplete.', flush=True)

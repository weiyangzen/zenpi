from pathlib import Path
import datetime, hashlib, json, os, shutil, signal, subprocess

R = Path(__file__).resolve().parents[2]
I = R / '.ops/stage1_execution/input-shutdown-current-3.1.21/root-integration'
D = R / '.ops/stage1_execution/input-shutdown-release-3.1.21'
assert not D.exists()
assert json.loads((I / 'verification.json').read_text())['status'] == 'passed'
D.mkdir()
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def save(p, value):
    p.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
inputs = json.loads((I / 'inputs-after-apply.json').read_text())
assert all(meta(R / p) == x for p, x in inputs.items())
save(D / 'inputs.json', inputs)
def run(name, argv, allowed=(0,), timeout=600):
    env = os.environ.copy()
    env.update(CARGO_NET_OFFLINE='true', CARGO_BUILD_JOBS='2', PYTHONDONTWRITEBYTECODE='1')
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    timed = False
    with (D / (name + '.log')).open('xb') as out:
        child = subprocess.Popen(argv, cwd=R, env=env, stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed = True
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=10)
            code = 124
    save(D / (name + '.run.json'), dict(argv=argv, cwd=str(R), started_at=started, ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=code, pid=child.pid, reaped=child.poll() is not None, timed_out=timed, log=meta(D / (name + '.log')), HOME_preserved=env.get('HOME') == os.environ.get('HOME'), CODEX_HOME_preserved=env.get('CODEX_HOME') == os.environ.get('CODEX_HOME')))
    assert all(meta(R / p) == x for p, x in inputs.items()), 'captured inputs drifted'
    print(name + ': exit ' + str(code), flush=True)
    assert code in allowed, name
    return code
# The authoritative unchanged harness builds release and records its first
# three launches. No prewarm, retry, threshold or timing boundary adjustment.
code = run('budget', ['python3', 'tools/bench_runtime.py', '--samples', '3', '--output', str(D / 'budget.json'), '--startup-evidence-dir', str(D / 'startup')], allowed=(0, 1))
budget = json.loads((D / 'budget.json').read_text())
assert code == (0 if budget['ok'] else 1)
binary = D / 'zenpi-release'
shutil.copy2(R / 'target/release/zenpi', binary)
binary.chmod(0o555)
assert meta(binary)['sha256'] == budget['cold_start']['binary_sha256']
save(D / 'binary.json', meta(binary))
# Exercise the user-critical top-plus folder picker and BentoBox against this
# immutable new binary, using separate newly created evidence directories.
for case in ['projects', 'bentobox']:
    run(case, ['python3', 'tools/stage1_host_smoke.py', '--binary', str(binary), '--case', case, '--report-dir', str(D / case)], timeout=300)
    result = json.loads((D / case / 'manifest.json').read_text())
    assert result['status'] == 'passed' and result['binary_sha256'] == meta(binary)['sha256']
save(D / 'master-verification.json', dict(complete=False, scope='Pending input closure product delta with production budget and project/BentoBox regressions; not whole Stage1 acceptance', current_binary=meta(binary), current_headless=inputs['src/headless.rs'], input_count=len(inputs), input_stable=True, budget_ok=budget['ok'], gates=budget['gates'], startup_samples_ms=budget['cold_start']['elapsed_ms']['samples'], rust_passed=95, rust_ignored=2, empty_lib_filter_not_counted=True, pty_cases=['projects', 'bentobox']))

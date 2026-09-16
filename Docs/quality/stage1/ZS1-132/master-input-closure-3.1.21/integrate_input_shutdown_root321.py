from pathlib import Path
import datetime, difflib, hashlib, json, os, signal, subprocess

R = Path(__file__).resolve().parents[2]
D = R / '.ops/stage1_execution/input-shutdown-current-3.1.21'
W = D / 'worker'
P = D / 'root-integration'
assert not P.exists()
P.mkdir()
def meta(path):
    b = path.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
old = R / 'src/headless.rs'
new = W / 'files/src/headless.rs'
assert meta(old)['sha256'] == '521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0'
assert meta(new)['sha256'] == 'bf252fec95a859b26f9aea402c918e9efbf6c2c55d3e24b403831f86d80ec06e'
assert (R / 'tests/headless_input_shutdown.rs').read_bytes() == (W / 'files/tests/headless_input_shutdown.rs').read_bytes()
assert json.loads((D / 'sealed-worker-verify.run.json').read_text())['exit_code'] == 0
keys = list(json.loads((D / 'inputs-after-baseline.json').read_text())) + ['tests/headless_input_shutdown.rs']
before = {rel: meta(R / rel) for rel in keys}
save(P / 'inputs-before.json', before)
patch = ''.join(difflib.unified_diff(old.read_text().splitlines(True), new.read_text().splitlines(True), fromfile='a/src/headless.rs', tofile='b/src/headless.rs'))
(P / 'product-only.patch').write_text(patch)
subprocess.run(['git', 'apply', '--check', str(P / 'product-only.patch')], cwd=R, check=True)
subprocess.run(['git', 'apply', str(P / 'product-only.patch')], cwd=R, check=True)
assert old.read_bytes() == new.read_bytes()
after = {rel: meta(R / rel) for rel in keys}
assert [rel for rel in keys if before[rel] != after[rel]] == ['src/headless.rs']
save(P / 'inputs-after-apply.json', after)
env = os.environ.copy()
env.update(CARGO_NET_OFFLINE='true', CARGO_BUILD_JOBS='2', PYTHONDONTWRITEBYTECODE='1')
env.pop('ZENPI_DOMAIN_STORE', None)
base = ['cargo', '+stable-aarch64-apple-darwin']
runs = [
    ('regression', base + ['test', '--offline', '--locked', '--jobs', '2', '--test', 'headless_input_shutdown', '--', '--test-threads=1']),
    ('related', base + ['test', '--offline', '--locked', '--jobs', '2', '--test', 'headless_protocol', '--test', 'headless_project_workspace', '--test', 'headless_domain_owner', '--test', 'resume_compact_owner', '--test', 'stage1_input_host', '--', '--test-threads=1']),
    ('tickets', base + ['test', '--offline', '--locked', '--jobs', '2', '--lib', 'input_queue::']),
    ('clippy', base + ['clippy', '--offline', '--locked', '--jobs', '2', '--all-targets', '--', '-D', 'warnings']),
    ('format', base + ['fmt', '--all', '--', '--check']),
]
for name, argv in runs:
    start = datetime.datetime.now(datetime.timezone.utc).isoformat()
    timed = False
    with (P / (name + '.log')).open('xb') as out:
        child = subprocess.Popen(argv, cwd=R, env=env, stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=600)
        except subprocess.TimeoutExpired:
            timed = True
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=10)
            code = 124
    save(P / (name + '.run.json'), dict(argv=argv, started_at=start, ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=code, pid=child.pid, reaped=child.poll() is not None, timed_out=timed, log=meta(P / (name + '.log'))))
    print(name + ': exit ' + str(code), flush=True)
    assert {rel: meta(R / rel) for rel in keys} == after, 'captured input drift during validation'
    assert code == 0, name
save(P / 'verification.json', dict(status='passed', inputs=after, product_only_patch=meta(P / 'product-only.patch'), new_test=meta(R / 'tests/headless_input_shutdown.rs'), runs=[name for name, _ in runs], scope='Pending input closure delta only; not full ZS1-084 or ZS1-132 acceptance'))

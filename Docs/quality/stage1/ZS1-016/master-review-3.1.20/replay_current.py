from pathlib import Path
import json, hashlib, os, shutil, subprocess, sys, time, signal, datetime
w = Path(__file__).resolve().parent
ROOT = w.parents[2]
source = Path('/Users/wangweiyang/GitHub/pi-mono/packages/ai')
runtime = Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/source016-runtime')
bun = Path('/Users/wangweiyang/.nvm/versions/node/v22.14.0/bin/bun').resolve()
sha = lambda data: hashlib.sha256(data).hexdigest()
canonical = lambda value: json.dumps(value, sort_keys=True, separators=(',', ':')).encode()

def inventory(root):
    rows = []
    for p in sorted(root.rglob('*')):
        if p.is_symlink():
            assert p.resolve().is_relative_to(root.resolve()), p
            rows.append(dict(path=p.relative_to(root).as_posix(), symlink=os.readlink(p)))
        elif p.is_file():
            data = p.read_bytes()
            rows.append(dict(path=p.relative_to(root).as_posix(), bytes=len(data), sha256=sha(data)))
    return rows

actual = w / 'actual-replay'
assert not actual.exists()
actual.mkdir()
source_before = inventory(source / 'src')
runtime_before = inventory(runtime / 'node_modules')
shutil.copytree(source / 'src', actual / 'ai/src', symlinks=True)
shutil.copy2(source / 'package.json', actual / 'ai/package.json')
shutil.copytree(runtime / 'node_modules', actual / 'node_modules', symlinks=True)
for name in ['package.json', 'package-lock.json']:
    shutil.copy2(runtime / name, actual / name)
assert inventory(actual / 'ai/src') == source_before
assert inventory(actual / 'node_modules') == runtime_before
probe = w / 'frozen-input/files/Docs/quality/stage1/ZS1-016/worker-source-probe/probe.ts'
original = probe.read_text()
old = '/Users/wangweiyang/GitHub/pi-mono/packages/ai/src/api/anthropic-messages.ts'
new = str(actual / 'ai/src/api/anthropic-messages.ts')
assert original.count(old) == 2
(actual / 'probe.ts').write_text(original.replace(old, new))
assert sha((actual / 'ai/src/api/anthropic-messages.ts').read_bytes()) == '8e105cc5b2dd304547ed613b70de4740c4db93cee830dc452c2de945052b12b7'
bun_hash = sha(bun.read_bytes())
version = subprocess.run([str(bun), '--version'], capture_output=True, text=True, check=True).stdout.strip()
identity = dict(source_files=source_before, runtime_files=runtime_before, bun=dict(path=str(bun), sha256=bun_hash, version=version),
    source_tree_sha256=sha(canonical(source_before)), runtime_tree_sha256=sha(canonical(runtime_before)),
    original_probe_sha256=sha(probe.read_bytes()), executed_probe_sha256=sha((actual / 'probe.ts').read_bytes()),
    adaptation='Only the two absolute source path literals are rebound to the byte-identical isolated copy.',
    provenance='Current full ai/src and local node_modules identities; no historical transitive dependency identity claim.')
(w / 'replay-inputs.json').write_text(json.dumps(identity, indent=2) + '\n')
env = os.environ.copy()
env['NODE_PATH'] = str(actual / 'node_modules')
argv = [str(bun), str(actual / 'probe.ts')]
start = time.perf_counter()
wall = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.Popen(argv, cwd=actual, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
timed_out = False
try:
    stdout, stderr = proc.communicate(timeout=20)
except subprocess.TimeoutExpired:
    timed_out = True
    os.killpg(proc.pid, signal.SIGKILL)
    stdout, stderr = proc.communicate()
elapsed = time.perf_counter() - start
(w / 'actual.stdout.json').write_bytes(stdout)
(w / 'actual.stderr.log').write_bytes(stderr)
receipt = dict(argv=argv, cwd=str(actual), started_at=wall, elapsed_seconds=elapsed, exit_code=proc.returncode,
    timeout=timed_out, environment_overrides={'NODE_PATH': env['NODE_PATH']}, HOME_preserved=True, CODEX_HOME_preserved=True,
    stdout_sha256=sha(stdout), stderr_sha256=sha(stderr))
(w / 'replay.run.json').write_text(json.dumps(receipt, indent=2) + '\n')
assert not timed_out and proc.returncode == 0, stderr.decode(errors='replace')
result = json.loads(stdout)
historical = json.loads((w / 'frozen-input/files/Docs/quality/stage1/ZS1-016/worker-source-probe/source-native-final.log').read_text())
assert result['scenarios'] == historical['scenarios']
assert len(result['scenarios']) == 23 and all(c['status'] == 'pass' for c in result['scenarios'])
assert result['http_requests'] == 24
assert inventory(source / 'src') == source_before and inventory(actual / 'ai/src') == source_before
assert inventory(runtime / 'node_modules') == runtime_before and inventory(actual / 'node_modules') == runtime_before
assert sha(bun.read_bytes()) == bun_hash
assert probe.read_text() == original
print(json.dumps(dict(actual_cases=23, actual_http_requests=24, exit_code=proc.returncode, elapsed_seconds=elapsed,
    current_source_files=len(source_before), runtime_files=len(runtime_before), bun_version=version,
    source_and_runtime_unchanged=True, historical_case_titles_match=True)))

from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-400'
D = R / '.ops/stage1_execution/directory400-current-3.1.21'
Q = R / 'Docs/quality/stage1/ZS1-400/master-review-3.1.21'
W = Path('/Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target400-directory-3.1.21-ready')
assert not D.exists() and not Q.exists()
D.mkdir()

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def read(p):
    return json.loads(p.read_text())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

def record(p):
    return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '805ac1beeab2d643a549a411574985d57db0757fd0a1be1dbef075390125e61d'
shutil.copytree(W, D / 'worker')
W = D / 'worker'
argv = ['python3', '-B', str(W / 'verify.py')]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.run(argv, cwd=R, capture_output=True, timeout=60)
(D / 'offline.stdout.json').write_bytes(proc.stdout)
(D / 'offline.stderr.log').write_bytes(proc.stderr)
dump(D / 'offline.run.json', dict(argv=argv, cwd=str(R), started_at=started,
     ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=proc.returncode,
     stdout=meta(D / 'offline.stdout.json'), stderr=meta(D / 'offline.stderr.log')))
assert proc.returncode == 0, proc.stderr
assert read(D / 'offline.stdout.json')['ok']
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
assert scope == 'target' and bp.items[KEY].state == '[ ]'
assert bp.items[KEY].depends == ('ZS1-099',) and bp.items['ZS1-099'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / report
assert meta(candidate)['sha256'] == 'f004a5b0b5095af3b70801601352129012b4b1d5cac501a7fff642ef616af84f'
entries = sorted((R / folder).iterdir())
assert [p.name for p in entries] == ['mio.rs', 'tty.rs']
assert all(p.is_file() and not p.is_symlink() for p in entries)
for p in entries:
    assert p.read_bytes() == (W / 'physical' / p.name).read_bytes()
contexts = read(W / 'context-read.json')
for row in contexts:
    assert (R / row['path']).read_bytes() == (W / 'context' / row['path']).read_bytes()
child = R / v.EVIDENCE / 'receipts/ZS1-099.master.json'
assert child.read_bytes() == (W / 'accepted099' / child.relative_to(R)).read_bytes()
receipt = read(child)
for artifact in receipt['artifacts']:
    v.artifact(R, artifact)
append = '''

## Controller independent directory acceptance — 3.1.21

The controller read the complete 20939-byte worker report and the entire portable verifier before executing it once in a new copied package. It verified 326 payloads without replaying any archived program. The controller independently enumerated the physical directory, checked the accepted099 receipt and all its artifact identities, and verified all15 context files against current main. This directory has exactly mio.rs and tty.rs, no direct subdirectory or symlink. Only099 is a frozen formal child. The already accepted099 complete229-line baseline,477-line previous implementation and178-line increment/current655-line understanding are reused through that exact receipt; a fresh whole-mio reread is not claimed.

For this directory review the controller newly read all278 lines of tty.rs, source.rs27, source/unix.rs11, sys.rs9, sys/unix.rs5, the Unix waker selector11 and both implementations28/34, read.rs436 including all15 tests, timeout.rs92 including all5 tests, file_descriptor.rs154, filter.rs115 including all5 tests, stream.rs146, root Cargo.toml50 and vendor Cargo.toml240. These are full semantic context reads, not extra formal file acceptances. Actual code confirms compile-time mutually exclusive backend/waker selection; constructor errors are erased by reader .ok(), with no fallback; source parser/token state and reader filtered/skipped queues have different owners. A successful filtered read restores locally skipped events, but an error through poll can drop read-local skipped events; poll's own skipped buffer remains stored until a later normal drain. Neither behavior is described as unconditional lossless recovery.

The tty alternative checks zero timeout before its parser queue, retries only Interrupted in read_complete, folds EOF/WouldBlock into zero, and does not contain the default mio pending-token/FIONREAD/idle-Esc behavior. Notification streams are nonblocking; the input TTY is not made nonblocking here. Signal registration IDs are discarded with a singleton comment; this file proves no explicit unregister-on-reset. FileDesc distinguishes borrowed stdin from owned /dev/tty, does not restore terminal mode, and does not retry a failed close. EventStream Drop sets shutdown and attempts wake but does not join its thread; ignored wake/poll errors do not prove bounded cancellation. These explicit static limits do not turn context-only alternative backends into accepted product configurations.

The worker's144 Rust and8/9 overlapping vendor checks bind its saved earlier41321e+944049 integration; the report preserves production_release_rebuilt=false at that historical capture. Later root6891 production results bind31ea+41321e. Current main has521c headless and its new durable version-selection regression; these are outside this directory and do not change any of the15 context files or mio/tty identities. New current-production validation is separate, so no old release evidence is relabeled. The socket failure and public PTY before/after both-pass counterevidence in accepted099 remain intact. No new runtime tests were needed for this report-only acceptance.

Accept only400 directory understanding of the frozen099-to-Unix-source integration. Parent401/402 and other ancestors, context-only tty/reader/stream files, full129/131 and whole Stage1 remain independently open. This acceptance establishes the documented ownership and error boundaries, not exhaustive platform behavior or durable terminal-input recovery. The worker report remains unchanged above; this master append qualifies its historical captured status. Rollback withdraws only400 report/receipt/status, never099 or product source.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['offline.stdout.json', 'offline.stderr.log', 'offline.run.json']:
    shutil.copy2(D / name, Q / name)
dump(Q / 'current-context.json', {row['path']: meta(R / row['path']) for row in contexts})
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file():
            archive.add(p, arcname=p.relative_to(W).as_posix(), recursive=False)
sizes = v.file_hashes(bp, R, Path(bp.header['source_repo']), target=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    dest = Q / 'authority-before' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(R / rel, dest)
(R / report).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / report, candidate.read_text() + append)
refs = [record(R / report)] + [record(Q / name) for name in ['review.md', 'worker-report.md', 'worker-manifest.json', 'worker-ready.tar.gz', 'offline.stdout.json', 'offline.run.json', 'current-context.json', 'accept.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
     baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='directory400-current-3.1.21',
     integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope,
     folder_path=folder, children=list(bp.items[KEY].depends), artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker A with independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Full directory report, portable verifier and15 complete current context files independently read. Exact accepted099 chain reused. Two physical files/no subdirectories; only099 formal child. Static alternative backend and cancellation limits preserved; accept400 only.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-400\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
    assert count == 1
    updated = v.parse(text)
    assert updated.requirement == bp.requirement
    g.atomic(R / v.BLUEPRINT, text)
    active = read(R / v.SELECTOR)
    active['snapshot_sha256'] = updated.snapshot
    g.atomic(R / v.SELECTOR, json.dumps(active, ensure_ascii=False, separators=(',', ':')) + '\n')
    for rel, (fields, rows) in v.scaffold(updated, sizes, v.EVIDENCE).items():
        g.atomic(R / rel, v.tsv(fields, rows).decode())
    front = v.frontiers(updated, read(R / v.EVIDENCE / 'claims.json')['claims'], sel['run_id'], 3)
    for todo in (R / v.EVIDENCE).glob('todos_*.md'):
        g.atomic(todo, v.todo(updated, front, v.BLUEPRINT, v.EVIDENCE + '/claims.json'))

state('[_]')
pre = v.validate(R, item=KEY)
dump(Q / 'before-promotion.json', pre)
assert pre['ok'], pre
state('[x]')
post = v.validate(R, item=KEY)
dump(Q / 'after-promotion.json', post)
assert post['ok'], post
argv = ['python3', 'tools/validate_stage1_blueprint.py', '--item', KEY]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.run(argv, cwd=R, capture_output=True, timeout=60)
(Q / 'gstage.stdout.log').write_bytes(proc.stdout)
(Q / 'gstage.stderr.log').write_bytes(proc.stderr)
dump(Q / 'gstage.run.json', dict(argv=argv, cwd=str(R), started_at=started,
     ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=proc.returncode,
     stdout=meta(Q / 'gstage.stdout.log'), stderr=meta(Q / 'gstage.stderr.log')))
assert proc.returncode == 0, proc.stderr
dump(Q / 'manifest.json', dict(item=KEY, master_accepted=True, artifacts={p.relative_to(Q).as_posix(): meta(p) for p in Q.rglob('*') if p.is_file()}))
print(json.dumps(dict(accepted=KEY, counts=post['counts'], snapshot=post['snapshot_sha256'])))

from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-401'
D = R / '.ops/stage1_execution/directory401-current-3.1.21'
Q = R / 'Docs/quality/stage1/ZS1-401/master-review-3.1.21'
W = Path('/Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target401-directory-3.1.21-ready')
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

assert meta(W / 'manifest.json')['sha256'] == 'b29b2f278ee0bfc956b6498f8e04bb5aef6e6d7b20c0449972435a3d0556d99d'
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
assert bp.items[KEY].depends == ('ZS1-400',) and bp.items['ZS1-400'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / report
assert meta(candidate)['sha256'] == '8bf1e0d11cacffbd39789e192cd12a180e82b0e6b60f66d0280328b78ef015be'
entries = sorted((R / folder).iterdir())
assert [p.name for p in entries] == ['unix', 'unix.rs', 'windows.rs']
assert all(not p.is_symlink() for p in entries)
assert entries[0].is_dir() and all(p.is_file() for p in entries[1:])
assert sorted(p.name for p in entries[0].iterdir()) == ['mio.rs', 'tty.rs']
for p in entries[1:]:
    assert p.read_bytes() == (W / 'physical' / p.name).read_bytes()
for p in entries[0].iterdir():
    assert p.read_bytes() == (W / 'prior400/physical' / p.name).read_bytes()
fresh = read(W / 'fresh-context-read.json')
reuse = read(W / 'reused-context-read.json')
contexts = fresh + reuse
for rows, prefix in [(fresh, 'fresh-context'), (reuse, 'prior400/context')]:
    for row in rows:
        assert (R / row['path']).read_bytes() == (W / prefix / row['path']).read_bytes()
child = R / v.EVIDENCE / 'receipts/ZS1-400.master.json'
receipt = read(child)
assert receipt['complete'] and receipt['manual_review']['decision'] == 'accepted'
for artifact in receipt['artifacts']:
    v.artifact(R, artifact)
assert read(W / 'dependency-state.json')['accepted'] is False
append = '''

## Controller independent directory acceptance — 3.1.21

The controller independently read this complete23446-byte report and its full portable verifier, then executed the verifier once in a fresh copied package:381 payloads verified without historical program execution. It intentionally verifies the frozen candidate's missing400 receipt; this historical result is retained. Since that capture, the controller has separately accepted400, with a real master receipt, full artifact chain and successful G-STAGE. Those current artifacts are independently rechecked and bound here before401 promotion. The earlier provisional state is not rewritten in the immutable worker package.

Physical source/ contains exactly unix.rs, windows.rs and unix/. No symlinks or additional entries exist. There are zero frozen direct files and one direct child directory400;099 is a grandchild and is not accepted again. For this review the controller newly read all100 lines of Windows source,48 lines of sys/windows.rs,86 of poll.rs,40 of waker.rs and378 of parse.rs in continuous1–205/206–378 ranges. All20 fresh/reused context identities were checked against main. The other15 complete context files were fully read by the controller immediately for400 and are now reused byte-for-byte; accepted099 retains its own complete baseline/current reading chain. This is an independent directory review, not another claim of full Windows file acceptance.

The code confirms that Windows owns a Console created from an input Handle, WinApiPoll, one pending UTF16 surrogate and mouse-button history; Unix owns its separate parser/readiness state through400. Ignored console records are consumed. Mouse parsing errors become None while button state still updates; unrelated non-key records do not clear the pending surrogate. Normal keys carry Press/Release, but surrogate completion uses KeyEvent::new. The old release-filter comment is weaker than the actual code. Best-effort keyboard layout lookup can differ from the layout at input time; its [0u16,16] array has two elements. Simultaneous mouse button transitions follow branch priority. These are documented static limits, not newly tested Windows failures.

WinApiPoll reacquires the current input Handle each call while Console retains its original construction identity. Count/read are separate operations, not a multi-reader transaction. Duration milliseconds narrow to u32, None means INFINITE, wake returns Interrupted after ignoring a semaphore-reset error, and parent reader maps Interrupted to false. Windows empty queue is not Unix EOF. Semaphore clones can extend resource lifetime; reset assignment preserves the prior value if creation fails. External WinAPI wrappers were not fully reviewed, so no exact CloseHandle or cross-platform cancellation guarantee is inferred. Source Drop neither restores mouse/raw mode nor joins EventStream's thread or persists a project draft. Compile-time source and waker selection, parent filtering ownership and the absence of fallback were independently checked in400 and reused here.

The report's earlier6891 production and144/68 Rust histories remain bound to their original snapshots. Current main521c+41321e has a separately archived92f95 release:80 Rust checks,55 actual CLI checks and8 original budgets pass in ZS1-132/master-explicit-version-3.1.21. These newer local product results are not needed to prove this directory's unchanged bytes and do not establish Windows behavior. No product, build, PTY, budget or archived runner was executed by this acceptance; no further context file is promoted.

Accept only401 directory understanding after the independently accepted400 dependency.402 and all other ancestors, complete084/117/129/131/132 and whole Stage1 remain open. The static alternate-platform limits remain explicit. Rollback withdraws this report/receipt/status only, without recursively withdrawing400/099 or touching source/user sessions.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['offline.stdout.json', 'offline.stderr.log', 'offline.run.json']:
    shutil.copy2(D / name, Q / name)
shutil.copy2(child, Q / 'dependency400.master.json')
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
refs = [record(R / report)] + [record(Q / name) for name in ['review.md', 'worker-report.md', 'worker-manifest.json', 'worker-ready.tar.gz', 'offline.stdout.json', 'offline.run.json', 'current-context.json', 'dependency400.master.json', 'accept.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
     baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='directory401-current-3.1.21',
     integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope,
     folder_path=folder, children=list(bp.items[KEY].depends), artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker A with independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Full401 report/verifier and5 complete Windows context files independently read;15 exact context reads reused from newly accepted400. Current400 dependency receipt/artifacts verified separately from historical missing state. Two context files and one formal child directory; accept401 only.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-401\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

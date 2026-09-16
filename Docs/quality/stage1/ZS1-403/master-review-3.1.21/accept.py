from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile
R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
KEY = 'ZS1-403'
D = R / '.ops/stage1_execution/directory403-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-403/master-review-3.1.21'
assert not Q.exists()
def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, x): p.write_text(json.dumps(x, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))
assert meta(W / 'manifest.json')['sha256'] == 'e823581c4325d8c6b0a6daa3facc9fda4c735872155779617145465a9966fed5'
assert read(D / 'offline.run.json')['exit_code'] == 0 and read(D / 'offline.stdout.json')['ok']
for x in read(W / 'manifest.json')['files']:
    assert meta(W / x['path']) == {k: x[k] for k in ['bytes', 'sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
assert scope == 'target' and bp.items[KEY].state == '[ ]'
assert bp.items[KEY].depends == ('ZS1-402',) and bp.items['ZS1-402'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / report
assert meta(candidate)['sha256'] == '6fbabcba8ddcf6e13c3132e7abb1e5ec4372d7c7083c1a6768260f217d8e884c'
inventory = read(W / 'physical-inventory.json')
entries = list((R / folder).iterdir())
assert sorted(p.name for p in entries) == sorted(x['name'] for x in inventory)
assert all(not p.is_symlink() for p in entries)
assert sum(p.is_file() for p in entries) == 10 and sum(p.is_dir() for p in entries) == 4
for x in inventory:
    p = R / x['path']
    if x['kind'] == 'file':
        assert meta(p) == {k: x[k] for k in ['bytes', 'sha256']}
    else:
        children = list(p.iterdir())
        assert sorted(q.name for q in children) == sorted(q['name'] for q in x['direct_children'])
        assert all(not q.is_symlink() for q in children)
current = read(W / 'related-source-identities.json')
for rel, x in current.items():
    assert meta(R / rel) == {k: x[k] for k in ['bytes', 'sha256']}
dep_path = R / v.EVIDENCE / 'receipts/ZS1-402.master.json'
dependency = read(dep_path)
assert dependency['complete'] and dependency['manual_review']['decision'] == 'accepted'
assert dependency['children'] == ['ZS1-401']
for artifact in dependency['artifacts']: v.artifact(R, artifact)
assert read(D / 'controller-fresh-read.json')['lines'] == 1892
append = '''

## Controller independent directory acceptance — 3.1.21

The controller independently read this entire 21170-byte report and its complete offline verifier. It newly read all eight context files: command.rs295, macros.rs378, cursor.rs504, terminal.rs568, tty.rs54, ansi_support.rs46, cursor/sys.rs20 and terminal/sys.rs27, totaling1892 lines including every source test. All eight current byte identities match the frozen full ranges. The25 complete prior contexts and accepted099655-line chain reuse the controller's actual402/401/400 readings with exact current hashes. No new whole-tree reading or Windows execution is claimed.

The physical src directory was separately enumerated:10 regular files and4 direct directories, without symlinks or extra hidden entries. The frozen scope has no formal direct files and only402/event as formal child. cursor/terminal selectors are necessary context; clipboard.rs, style.rs and style/ remain inventory-only. The worker's initial402-missing state is preserved. The controller now checks the actual402 accepted receipt and every one of its9 artifacts; this prerequisite is current external evidence, not a rewrite of the old package. Its portable checker ran once in a fresh copied package with external logs and verified479 payloads,10 contiguous fresh read blocks and25 exact reused context files. Integrity verification supports, but does not replace, this independent semantic review.

Command is an output interface, not an internal command queue. QueueableCommand writes immediately to the supplied writer; execute flushes after successful queueing. Windows unsupported-ANSI fallback first flushes and can invoke the current console API rather than the supplied writer. queue! short-circuits later command expressions on failure; execute! can evaluate its writer expression again for flush. Neither partial bytes nor terminal effects roll back after an I/O error. The fmt adapter returns recorded I/O errors but panics if a Command returns fmt::Error without an underlying write failure. impl_display may invoke legacy Windows APIs, so Command Display is not universally a pure serialization operation. osc! uses ST despite its BEL comment; SetTitle formats BEL independently.

sync_update queues Begin, calls the closure, then executes End. A closure-returned Result is nested in the outer Result and End still runs on normal return; a panic has no End guard. Begin failure skips the closure and End/flush failure replaces its returned value. Synchronized frames do not switch alternate screens and do not synchronize input, persistence or project ownership. Commands for screen, wrapping, clearing, scrolling and cursor visibility have no automatic Drop cleanup. Cursor absolute encoders add1 to u16, so maximal values are not uniformly safe; relative zero arguments also have distinct platform behavior. Save/Restore target terminal state rather than a per-project Rust snapshot. Windows blinking/style methods can succeed without visible action.

The crate module root and feature/target selectors connect output commands, event types, cursor queries, raw mode and tty detection without making them one owner. The global event reader, saved-Termios mutex, source parser/readiness and terminal display are separate. Raw registration is not reference counting or proof of current external termios state. IsTty borrows the handle and collapses query failure to false. Windows ANSI capability initialization can set a console-mode bit, is cached once and does not restore or recompute after handle/TERM changes. Its expression tries enable_vt_processing before TERM despite the comment. These are static findings, not runtime platform coverage.

The402 query limitations remain:2000ms per poll is not a function-wide bound, later DA reads may be unbounded, and size fallback can wait on tput. reset_event_reader does not flush OS input, restore raw/display modes, persist drafts or join every caller. Public synchronous/async reader restrictions and EventStream's unjoined Drop remain. Cursor tests are ignored, terminal resize is ignored and raw-mode test can return early when enabling fails. Merely seeing test functions or historical passing totals cannot prove the terminal actually performed those operations. No product/runtime/Cargo/PTY/budget or archived runner was executed for this403 review.

Accept only403's complete understanding of its frozen directory boundary after the independent402 acceptance.404/405/root, inventory-only files, unlisted Windows implementations and product131 remain open. The known zero-coordinate and multi-digit-flags input parser issues are not fixed by this directory report; a separate worker repair turn was interrupted by the platform and its partial work is not accepted here. The concurrent headless input-closure integration is likewise separate from this directory's scope. Existing BentoBox and top-plus behavior are preserved. Rollback withdraws only403 report/receipts/state and retains every child receipt, historical failure and product/user byte.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
shutil.copy2(dep_path, Q / 'dependency402.master.json')
for name in ['offline.stdout.json', 'offline.stderr.log', 'offline.run.json', 'controller-fresh-read.json']:
    shutil.copy2(D / name, Q / name)
dump(Q / 'current-context.json', current)
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file(): archive.add(p, arcname=p.relative_to(W).as_posix(), recursive=False)
sizes = v.file_hashes(bp, R, Path(bp.header['source_repo']), target=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    p = Q / 'authority-before' / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(R / rel, p)
(R / report).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / report, candidate.read_text() + append)
names = ['review.md', 'worker-report.md', 'worker-manifest.json', 'worker-ready.tar.gz', 'offline.stdout.json', 'offline.run.json', 'current-context.json', 'controller-fresh-read.json', 'dependency402.master.json', 'accept.py']
refs = [record(R / report)] + [record(Q / name) for name in names]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement, baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='directory403-current-3.1.21', integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope, folder_path=folder, children=list(bp.items[KEY].depends), artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker A with independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'), findings='Complete403 report/verifier and1892 new context lines independently read;25 prior contexts and099 chain exact reuse. Actual402 receipt9 artifacts verified;479 offline payloads pass.10files/4dirs, only402 formal child.Only403 frozen scope accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')
def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-403\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
    assert count == 1
    updated = v.parse(text)
    assert updated.requirement == bp.requirement
    g.atomic(R / v.BLUEPRINT, text)
    active = read(R / v.SELECTOR)
    active['snapshot_sha256'] = updated.snapshot
    g.atomic(R / v.SELECTOR, json.dumps(active, ensure_ascii=False, separators=(',', ':')) + '\n')
    for rel, (fields, rows) in v.scaffold(updated, sizes, v.EVIDENCE).items():
        g.atomic(R / rel, v.tsv(fields, rows).decode())
    claims = read(R / v.EVIDENCE / 'claims.json')
    front = v.frontiers(updated, claims['claims'], sel['run_id'], 3)
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
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
check = subprocess.run(argv, cwd=R, capture_output=True, timeout=60)
(Q / 'gstage.stdout.log').write_bytes(check.stdout)
(Q / 'gstage.stderr.log').write_bytes(check.stderr)
dump(Q / 'gstage.run.json', dict(argv=argv, exit_code=check.returncode, started_at=start, ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat()))
assert check.returncode == 0, check.stderr
dump(Q / 'manifest.json', dict(item=KEY, master_accepted=True, artifacts={p.relative_to(Q).as_posix():meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main()
print(json.dumps(dict(accepted=KEY, counts=post['counts'], snapshot=post['snapshot_sha256'])))

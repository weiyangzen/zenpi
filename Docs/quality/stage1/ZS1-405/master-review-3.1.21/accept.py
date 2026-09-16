from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile
R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
KEY = 'ZS1-405'
D = R / '.ops/stage1_execution/directory405-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-405/master-review-3.1.21'
assert not Q.exists()
def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, x): p.write_text(json.dumps(x, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))
assert meta(W / 'manifest.json')['sha256'] == 'ec13b6e0f2b89be32740d1e4db059a442f3d5d0c768a4bfed1da55bc63edf4dc'
assert read(D / 'offline.run.json')['exit_code'] == 0 and read(D / 'offline.stdout.json')['ok']
for x in read(W / 'manifest.json')['files']:
    assert meta(W / x['path']) == {k: x[k] for k in ['bytes', 'sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
assert scope == 'target' and bp.items[KEY].state == '[ ]'
assert bp.items[KEY].depends == ('ZS1-404',) and bp.items['ZS1-404'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / report
assert meta(candidate)['sha256'] == 'c16cdca6fdc1350da3c8c3cfd29343f75e8511dd16be19532fb12bce3eb56f0a'
inventory = read(W / 'physical-inventory.json')
entries = list((R / folder).iterdir())
assert sorted(p.name for p in entries) == sorted(x['name'] for x in inventory)
assert all(not p.is_symlink() for p in entries)
assert sum(p.is_file() for p in entries) == 0 and sum(p.is_dir() for p in entries) == 1
for x in inventory:
    p = R / x['path']
    if x['kind'] == 'file':
        assert meta(p) == {k: x[k] for k in ['bytes', 'sha256']}
    else:
        children = list(p.iterdir())
        assert sorted(q.name for q in children) == sorted(q['name'] for q in x['direct_children'])
        assert all(not q.is_symlink() for q in children)
        for child in x['direct_children']:
            if child['kind'] == 'file': assert meta(R / child['path']) == {k:child[k] for k in ['bytes','sha256']}
current = {x['path']: {k:x[k] for k in ['bytes','sha256']} for name in ['new-context-read.json','reused-complete-context.json','reused-partial-context.json'] for x in read(W / name)}
current['vendor/crossterm/src/event/source/unix/mio.rs'] = {k:read(W / 'capture.json')['mio_identity'][k] for k in ['bytes','sha256']}
for rel, x in current.items():
    assert meta(R / rel) == {k: x[k] for k in ['bytes', 'sha256']}
dep_path = R / v.EVIDENCE / 'receipts/ZS1-404.master.json'
dependency = read(dep_path)
assert dependency['complete'] and dependency['manual_review']['decision'] == 'accepted'
assert dependency['children'] == ['ZS1-403']
for artifact in dependency['artifacts']: v.artifact(R, artifact)
assert read(D / 'controller-fresh-read.json')['lines'] == 337
append = '''

## Controller independent directory acceptance — 3.1.21

The controller independently read the complete10907-byte405 report, its full portable verifier and all10 specified current-context blocks. The source excerpts cover270 lines of TUI imports, both event-loop poll/read call sites, editor callbacks and complete suspend/resume methods. Root lock excerpts cover67 lines,20 already read for404 and47 new lines identifying ratatui, ratatui-crossterm and zenpi. This totals337 directly inspected lines with317 newly read lines; neither full TUI nor full root lock is claimed.42 complete contexts plus3 explicitly partial contexts reuse the controller's actual404/403/402/401/400 readings by exact byte identity.099's current655-line mio chain is separately verified.

The root independently enumerates vendor as zero regular files and one direct directory,crossterm, without a symlink or hidden extra entry. Its child's direct9file/4directory inventory and file hashes match404. There is no vendor-level Cargo manifest or root .cargo/config in the inspected project paths. This says nothing about uninspected user/global configuration. The frozen scope has no formal direct file and exactly404 as its child. The actual now-accepted404 receipt and all11 artifacts are checked; the original package's missing403/404 snapshot and initial absent worker/vendor read failure remain historical evidence. The copied545-payload package passed its new read-only verifier once from /tmp with external logs. That integrity result does not substitute for this semantic review.

Root Cargo's patch explicitly points crossterm to vendor/crossterm, not vendor itself or every descendant. The direct dependency leaves defaults enabled and adds bracketed-paste. Ratatui's crossterm backend creates a second reference to the same locked crate: zenpi→ratatui0.30.2→ratatui-crossterm0.1.2→crossterm0.29.0. The selected crossterm lock block has no registry source/checksum. These text connections are not a new Cargo resolution/build result. The vendor lock is a separate development graph and does not replace the host lock. Root's libc dependency does not by itself select crossterm's optional libc feature. Prior404's normalized/original manifest distinction and stale README/example paths remain qualified.

The new TUI excerpts concretely show imported cursor/event/raw/alternate-screen primitives and CrosstermBackend. Two host loops call poll then read and pass the event into TuiState; resize/render/project ownership stays in the host. These snippets do not establish that every host has identical behavior or complete the large TUI file's review. External-editor startup/resume paths explicitly pass reset_event_reader into TerminalGuard. Suspension resets the internal reader, flushes through the original terminal, sets active=false, and evaluates screen,raw andoriginal restore actions before combining their Results. Errors do not make these effects transactional. Resume reclaims, resets, flushes, restores, enables raw, sets active before output and resizes. Its successive question-mark returns can leave already completed effects; the active flag allows later leave handling but is not blanket rollback proof.

Event reader reset, OS input flush, raw state, output terminal modes, foreground process ownership and persisted drafts are separate operations. The function-pointer callback does not itself prove exclusive cross-thread ownership. The vendor container creates none of those owners and has no program entry point, session/WAL or provider lifecycle. Known parser/query/Drop limits stay with their independently reviewed child scopes; they are not fixed or runtime-tested by this report. No product/Cargo/PTY/budget or archived runner was executed for405. Current headlessbf252 observed by the worker is identity-only in its405 report; the root's separate shutdown-input product review must not be relabeled as vendor semantic evidence.

Accept405 only after independent404 acceptance. This closes the frozen vendor-container directory understanding, not091/root or every product feature depending on it. Whole TUI/headless, all third-party configurations, terminal-editor fault paths and131 remain independently open. The current d0cc production first-start1066.50ms still fails its1000ms gate, even though95 Rust and23 selected PTY checks passed in the separately published product integration. No result is retried, weakened or made a405 test. Rollback withdraws only405 report/receipts/state and preserves all child acceptances, product bytes, historical failures and user state.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
shutil.copy2(dep_path, Q / 'dependency404.master.json')
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
names = ['review.md', 'worker-report.md', 'worker-manifest.json', 'worker-ready.tar.gz', 'offline.stdout.json', 'offline.run.json', 'current-context.json', 'controller-fresh-read.json', 'dependency404.master.json', 'accept.py']
refs = [record(R / report)] + [record(Q / name) for name in names]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement, baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='directory405-current-3.1.21', integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope, folder_path=folder, children=list(bp.items[KEY].depends), artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker A with independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'), findings='Complete405 report/verifier and337 explicit context lines read,317new/20rechecked.42 prior complete and3 partial contexts exact reuse. Actual404 receipt11 artifacts verified;545 offline payloads pass.0files/1dir, only404 formal child.Only405 frozen container scope accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')
def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-405\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

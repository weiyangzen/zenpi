from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile
R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
KEY = 'ZS1-404'
D = R / '.ops/stage1_execution/directory404-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-404/master-review-3.1.21'
assert not Q.exists()
def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, x): p.write_text(json.dumps(x, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))
assert meta(W / 'manifest.json')['sha256'] == '1290e7e4a02df8bbc0636b042ada3ee02cacb8bb71da51c4e5d04db8ddccf99e'
assert read(D / 'offline.run.json')['exit_code'] == 0 and read(D / 'offline.stdout.json')['ok']
for x in read(W / 'manifest.json')['files']:
    assert meta(W / x['path']) == {k: x[k] for k in ['bytes', 'sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
assert scope == 'target' and bp.items[KEY].state == '[ ]'
assert bp.items[KEY].depends == ('ZS1-403',) and bp.items['ZS1-403'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / report
assert meta(candidate)['sha256'] == '14d0d361b432c5ac3f47fe75debde04c049b966dbf8f77bbe57825727a1e5260'
inventory = read(W / 'physical-inventory.json')
entries = list((R / folder).iterdir())
assert sorted(p.name for p in entries) == sorted(x['name'] for x in inventory)
assert all(not p.is_symlink() for p in entries)
assert sum(p.is_file() for p in entries) == 9 and sum(p.is_dir() for p in entries) == 4
for x in inventory:
    p = R / x['path']
    if x['kind'] == 'file':
        assert meta(p) == {k: x[k] for k in ['bytes', 'sha256']}
    else:
        children = list(p.iterdir())
        assert sorted(q.name for q in children) == sorted(q['name'] for q in x['direct_children'])
        assert all(not q.is_symlink() for q in children)
current = {x['path']: {k:x[k] for k in ['bytes','sha256']} for name in ['new-context-read.json','reused-context-read.json'] for x in read(W / name)}
current['vendor/crossterm/src/event/source/unix/mio.rs'] = {k:read(W / 'reused-mio-identity.json')[k] for k in ['bytes','sha256']}
for rel, x in current.items():
    assert meta(R / rel) == {k: x[k] for k in ['bytes', 'sha256']}
dep_path = R / v.EVIDENCE / 'receipts/ZS1-403.master.json'
dependency = read(dep_path)
assert dependency['complete'] and dependency['manual_review']['decision'] == 'accepted'
assert dependency['children'] == ['ZS1-402']
for artifact in dependency['artifacts']: v.artifact(R, artifact)
assert read(D / 'controller-fresh-read.json')['lines'] == 818
append = '''

## Controller independent directory acceptance — 3.1.21

The controller read this entire21348-byte report, full portable verifier and every one of the26 new context blocks. Nine complete files total537 lines: Cargo.toml.orig122, VCS metadata6, gitignore5, Travis42, LICENSE21, README222, known-problems14, CONTRIBUTING65 and example README40. Three explicitly partial files add281 lines: CHANGELOG1–95 plus vendor/root lock headers and the complete selected package blocks. The remaining changelog/lock contents and example Rust programs were not newly read. Captured full-file hashes establish identity, not an assertion that unread bytes were understood.33 complete prior contexts and accepted099's655-line chain reuse actual root403/402/401/400 readings with exact current hashes.

The controller independently enumerated9 regular files and4 direct directories, including hidden entries, and checked the direct children and captured file identities. The frozen scope has zero formal direct files and only403/src as formal child; .github/docs/examples remain context. The current403 master receipt and every one of its11 artifacts are individually checked. The worker's frozen initial402/403-missing state remains untouched; current dependency acceptance is separately attached. A copied fresh package's new read-only verifier ran once from /tmp, with external output, and verified524 payloads. It checks read ranges, package/lock selections, prior chains, inventory and the report-only patch, not semantic or runtime completion.

Both manifest forms declare crossterm0.29.0, edition2021, rust-version1.63.0 and src/lib.rs. Actual Cargo.toml is the normalized build input; Cargo.toml.orig describes the authored configuration but does not independently control a build. Explicit lib/ten example targets and disabled automatic discovery delimit this snapshot. build=false does not establish that all transitive dependencies lack build scripts; autotests=false does not disable source cfg(test) modules. Metadata all-features is documentation intent, not the application's feature selection or a measured MSRV claim. VCS metadata stays at its upstream value despite local event/reader edits. LICENSE and attribution remain byte-preserved; no new distribution/legal conclusion is made.

The root Cargo patch chooses this local crate and the root lock, not its standalone development lock. The selected lock blocks really differ: mio1.0.3→1.2.3, rustix1.0.5→1.1.4, signal-hook0.3.17→0.3.18 and signal-hook-mio0.2.4→0.2.5 between vendor and host graphs. Presence of optional/dev packages in a lock is not proof that they are activated in a production invocation. The root crossterm entry has no registry source/checksum. The vendor lock and gitignore/package-exclude can coexist on disk; exclusion rules do not remove the physical file. Locks were read at the declared header/package ranges without resolving/updating dependencies or verifying registry checksums online.

The selected defaults include bracketed-paste/events/windows/derive-more, while target cfg still chooses Unix behavior on this host. events enables mio/signal dependencies; use-dev-tty changes the source/waker selectors and does not automatically remove those dependency declarations or become an initialization-error fallback. Root's direct libc dependency does not itself enable the vendored crate's libc feature. EventStream, serde, osc52 and dev dependencies retain their distinct gates. Package metadata and configuration do not create a session, provider, WAL or draft owner. Input reader/parser/readiness, raw-mode registration and output Command state remain separate, with the403 limitations preserved: no automatic screen restoration, no transactional output rollback or panic guard, and no global bounded query guarantee.

Two concrete documentation drifts are independently confirmed: README's Cargo snippets still name0.27 and its filedescriptor/libc feature account does not match the current manifest/selectors; examples README points to interactive-test/interactive-demo directories absent from the actual tree. These are understood documentation defects, not newly executed product failures or reasons to invent successful platform coverage. Travis commands, contributor guidance and known-problem attributions were read as historical development material, not executed as instructions. The declared examples and source tests do not prove current terminal effects. No Cargo/build/PTY/provider/runtime/budget or archived runner was executed for404.

Accept404 only after independent403 acceptance. This is understanding of the frozen package boundary and its real limits.405/vendor and091/root remain separate open directories; inventory-only documentation/assets, ten example programs, unlisted Windows internals and product131 do not become accepted. The separate currentheadless bf252 repair and d0cc release have95 Rust and23 PTY checks but a failed1066.50ms cold-start gate; none of these results are repackaged as404 tests or a full-stage pass. Root rollback withdraws404 report/receipts/state only, retaining child acceptance, product source, all historical failures and user state.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
shutil.copy2(dep_path, Q / 'dependency403.master.json')
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
names = ['review.md', 'worker-report.md', 'worker-manifest.json', 'worker-ready.tar.gz', 'offline.stdout.json', 'offline.run.json', 'current-context.json', 'controller-fresh-read.json', 'dependency403.master.json', 'accept.py']
refs = [record(R / report)] + [record(Q / name) for name in names]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement, baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='directory404-current-3.1.21', integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope, folder_path=folder, children=list(bp.items[KEY].depends), artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker A with independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'), findings='Complete404 report/verifier and818 explicit new context lines independently read;33 prior complete contexts and099 chain exact reuse. Actual403 receipt11 artifacts verified;524 offline payloads pass.9files/4dirs, only403 formal child.Only404 frozen package scope accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')
def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-404\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

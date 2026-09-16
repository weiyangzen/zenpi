from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-406'
F = R / '.ops/stage1_execution/tests406-current-3.1.21'
D = R / '.ops/stage1_execution/tests406-current-final-3.1.21'
Q = R / 'Docs/quality/stage1/ZS1-406/master-review-3.1.21'
W = Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-406-tests321-ready')
assert F.is_dir() and not D.exists() and not Q.exists()
D.mkdir()
for name in ['frozen-verifier.stdout.json','frozen-verifier.stderr.log','frozen-verifier.run.json']:
    shutil.copy2(F / name, D / name)

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def read(p):
    return json.loads(p.read_text())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

def record(p):
    return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '12b878976708cbf250fe84e30617e03534408a0c66c10c36c2f3db04dc8d8e8d'
W = F / 'worker'
assert meta(W / 'manifest.json')['sha256'] == '12b878976708cbf250fe84e30617e03534408a0c66c10c36c2f3db04dc8d8e8d'
old_audit = read(D / 'frozen-verifier.stdout.json')
assert read(D / 'frozen-verifier.run.json')['exit_code'] == 1
assert {c['name'] for c in old_audit['checks'] if not c['passed']} == {
    '69 copied and live direct identities', 'final captured selector and blueprint current',
    '15 initial context snapshots and explicit live overlays', 'last observed Gantt revision explicitly preserved',
    '224 initial input capture with explicit live overlays'}
for row in read(W / 'manifest.json')['files']:
    assert meta(W / row['path']) == {k:row[k] for k in ['bytes','sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
assert scope == 'target' and bp.items[KEY].state == '[ ]'
assert bp.items[KEY].depends == ('ZS1-133',) and bp.items['ZS1-133'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / 'files' / report
assert meta(candidate)['sha256'] == '17cd7ee5d2f7c0c0f8dc78a2b776d18de85c7637004d53b05e8eb964d2d1da32'
inventory = read(W / 'direct-inventory.json')
entries = sorted((R / folder).iterdir())
assert len(entries) == 69 and all(p.is_file() and not p.is_symlink() for p in entries)
assert {p.relative_to(R).as_posix() for p in entries} == {row['path'] for row in inventory}
changes = {}
for row in inventory:
    expected = {k:row[k] for k in ['bytes','sha256']}
    assert meta(W / 'current' / row['path']) == expected
    actual = meta(R / row['path'])
    if actual != expected:
        changes[row['path']] = dict(frozen=expected, current=actual)
assert set(changes) == {'tests/headless_domain_owner.rs'}
assert changes['tests/headless_domain_owner.rs']['current']['sha256'] == '6f121e2ea3e1f7f0ffc19d8470f94662163b1eb1e78cd3a71c97975ad06361e6'
assert (R / 'tests/headless_domain_owner.rs').read_bytes().startswith((W / 'current/tests/headless_domain_owner.rs').read_bytes())
inputs = read(W / 'current-inputs.json')
overrides = {row['path']:row['live'] for row in read(W / 'postcapture-live-input-delta.json')}
overrides.update({row['path']:row['latest'] for row in read(W / 'latest-observed-delta.json')})
current = {path:meta(R / path) for path in inputs}
delta = {path:dict(frozen=overrides.get(path, old), current=current[path]) for path,old in inputs.items() if current[path] != overrides.get(path,old)}
assert set(delta) == {'src/headless.rs', 'tests/headless_domain_owner.rs', 'tools/generate_stage1_gantt.py'}
assert current['src/headless.rs']['sha256'] == '521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0'
contexts = read(W / 'context-inventory.json')
for row in contexts:
    assert meta(W / 'current' / row['path']) == {k:row[k] for k in ['bytes','sha256']}
    expected = overrides.get(row['path'], {k:row[k] for k in ['bytes','sha256']})
    if row['path'] not in delta:
        assert meta(R / row['path']) == expected
child = R / v.EVIDENCE / 'receipts/ZS1-133.master.json'
assert child.read_bytes() == (W / 'dependency133/receipt.json').read_bytes()
receipt = read(child)
for artifact in receipt['artifacts']:
    v.artifact(R, artifact)
oldbp = v.parse((W / 'authority-final' / v.BLUEPRINT).read_text())
assert bp.requirement == oldbp.requirement
for key in ['ZS1-133', 'ZS1-406', 'ZS1-091']:
    assert bp.items[key] == oldbp.items[key]
dump(D / 'controller-current-delta.json', dict(direct_file_changes=changes, input_changes=delta,
    source_inputs=current, frozen_verifier_failed_checks=5, preserved_failed_audit=True,
    scope='406 only, exact current delta qualification; not global gate modification'))
for path in delta:
    dest = D / 'current-delta' / path
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_bytes((R / path).read_bytes())
append = '''

## Controller independent directory acceptance — 3.1.21

The controller independently read the complete26865-byte directory report and entire offline verifier. The verifier was run once in a fresh copied package and correctly exited1: five checks comparing frozen metadata with current live state no longer hold. The exact failed result is preserved. A new, separate current-delta audit verifies every original144 payload and all133 receipt artifacts; confirms the same69 regular direct files/no subdirectories; and proves the only changed test file is context-only headless_domain_owner.rs. Across224 captured inputs, the only differences from the package's final overlays are this test, headless.rs and the Gantt generator. Current requirement and individual133/406/091 obligations are unchanged. No arbitrary drift was suppressed and no old verifier or runtime program was rerun to turn this result green.

The original583-line plus404-line current133 semantic review is reused through the exact accepted receipt, complete987-line8375 source identity and archived report chain. The controller read its master review again and newly inspected133 helper/provider/deadline/cleanup ranges1–268 and435–500, all project_workspace.rs model tests and mode_boundary.rs, complete lib.rs/main.rs and CI configuration, TUI project helper1–105, protocol1–80/2394–2444, resources1–45/tools1–55, project owner380–445/500–850, protocol140–165/310–347, slash_actions210–285 and current headless stream wrapper/explicit project-cwd diff connection. These context ranges establish actual library versus CLI/TestBackend/PTY boundaries and owner publication/persistence ordering without accepting those sibling files. Root Cargo was fully read in the preceding directory review and its unchanged identity reused.

133 owns private Unix fixtures, two streams and a host thread; Wire Drop only half-closes input and does not join. A per-read timeout is not an overall response-loop/join deadline. The added busy test runs in a separate process group with45-second outer bound and bounded provider gates/body; successful fixtures are removed while stdout retains real records. Four B-content observations happen before release, with a fifth response being cache replay; final reporting order must not be confused with sampling order. The child1-test summary is already included in13, so old13+43 remains56. No deterministic summary/model semantics or all timeout branches are inferred. Protocol and resource/tool test modules differ in helper design and import style; the directory is not uniformly black-box or governed by one shared timeout helper.

The project pool prepares a candidate owner and mappings, writes a bounded/stale-writer-checked checkpoint, then publishes memory. Resume commits the mapping before caller-owned Agent replacement. The file uses O_NOFOLLOW/flock, create_new temporary files, file sync and rename; the inspected code does not fsync the parent directory. A failed preparation can leave independent session artifacts, and stronger crash/cross-process guarantees are not inferred from metadata tests. Explicit project.cwd drives local busy diff even while the Agent mutex is held elsewhere. This is why133 checks actual Git content and journal/owner identity as well as response labels. The entire product state machine remains separately owned.

The new context-only headless_domain_owner increment is74 lines, extending the original329-line source to403 lines, exact6f121e identity. The controller previously read the complete original file, authored this six-case durable version-selection regression and read the full added test again here; original bytes remain an exact prefix. Before product repair it actually failed, after521c it passed among80 top-level Rust checks. That80/55 actual production CLI/8 budget evidence belongs to92f95 and is independently archived in ZS1-132/master-explicit-version-3.1.21. The test seeds a real temporary DomainStore, drives run_headless and checks durable exact-version association/no mutation after rejection; it does not expand406's sole formal child133 or constitute a Google model-version test. Existing context-test environment and platform limitations are not erased by this append.

The worker's old944049+1afa56 tests, initial41321e capture and later31ea cleanup explanation remain historical. Root separately reviewed/integrated the subsequent explicit-version521c change and earlier shutdown cleanup; those current differences are preserved as exact snapshots, not relabeled as the old56 execution. A host join still does not imply detached cleanup hooks have finished. Permanent non-cooperation and late independent writers remain unproved. Gantt changes report progress only. No test/build/PTY/budget execution occurs in this report acceptance.

Accept406 directory understanding only after accepted133. The other68 physical tests remain context-only, even where particular helper code was read. No091/root, sibling test, full084/117/123/132 or entire-stage acceptance follows. Rollback withdraws406 report/receipt/status and preserves133, all source changes, failure evidence and user sessions. The new master append qualifies frozen snapshots rather than rewriting the worker's historical report.
'''
Q.mkdir(parents=True)
shutil.copy2(R / '.ops/stage1_execution/accept_tests406_321.py', Q / 'initial-accept-attempt.py')
(Q / 'controller-prepare-failure.md').write_text('The first controller acceptance attempt exited1 before publication because it incorrectly required CI/release workflow context to be included in the224 build-input map. Workflows are separately captured context. The new attempt checks both exact workflow identities directly; no source/test/gate threshold changed, and no prior verifier/product program was rerun. Original script preserved.\n')
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['frozen-verifier.stdout.json', 'frozen-verifier.stderr.log', 'frozen-verifier.run.json', 'controller-current-delta.json']:
    shutil.copy2(D / name, Q / name)
shutil.copytree(D / 'current-delta', Q / 'current-delta')
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
refs = [record(R / report)] + [record(Q / name) for name in ['review.md', 'worker-report.md', 'worker-manifest.json', 'worker-ready.tar.gz', 'frozen-verifier.stdout.json', 'frozen-verifier.run.json', 'controller-current-delta.json', 'current-context.json', 'accept.py']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
     baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='tests406-current-3.1.21',
     integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope,
     folder_path=folder, children=list(bp.items[KEY].depends), artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker B with independent controller current-delta qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Full406 report/verifier and actual test/owner boundary context read; accepted133 full source chain reused. Frozen audit5 live-drift failures preserved, new exact current-delta audit permits only known headless/test/Gantt changes.69files/no directories,only133formalchild; accept406 only.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-406\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

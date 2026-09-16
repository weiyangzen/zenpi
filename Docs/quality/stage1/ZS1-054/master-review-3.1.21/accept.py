from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-054'
D = R / '.ops/stage1_execution/harness054-current-3.1.21'
Q = R / 'Docs/quality/stage1/ZS1-054/master-review-3.1.21'
W = Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-054-harness321-ready')
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

assert meta(W / 'manifest.json')['sha256'] == '8f5cbc32c1b6d4b622b01ac320f2d881509337ad114d8ba3e1744851b050926d'
shutil.copytree(W, D / 'worker')
W = D / 'worker'
argv = ['python3', '-B', str(W / 'verify_offline.py'), str(W)]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.run(argv, cwd=R, capture_output=True, timeout=60)
(D / 'offline.stdout.json').write_bytes(proc.stdout)
(D / 'offline.stderr.log').write_bytes(proc.stderr)
dump(D / 'offline.run.json', dict(argv=argv, cwd=str(R), started_at=started,
     ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=proc.returncode,
     stdout=meta(D / 'offline.stdout.json'), stderr=meta(D / 'offline.stderr.log')))
assert proc.returncode == 0, proc.stderr
assert read(D / 'offline.stdout.json')['passed']
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
assert scope == 'source' and bp.items[KEY].state == '[ ]'
assert bp.items[KEY].depends == ('ZS1-050', 'ZS1-051') and all(bp.items[k].state == '[x]' for k in bp.items[KEY].depends)
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / 'files' / report
assert meta(candidate)['sha256'] == 'd6ed82e57d1984d6061c49546ed5de7e9639d241ab12d788f9b171348d2e4295'
upstream = Path(bp.header['source_repo'])
inventory = read(W / 'direct-inventory.json')
entries = sorted((upstream / folder).iterdir())
assert {p.name for p in entries} == {row['name'] for row in inventory}
assert all(not p.is_symlink() for p in entries)
assert sum(p.is_file() for p in entries) == 12 and sum(p.is_dir() for p in entries) == 7
for row in inventory:
    if row['kind'] == 'file':
        assert meta(upstream / row['path']) == {k: row[k] for k in ['bytes', 'sha256']}
contexts = read(W / 'context-source-inventory.json')
for row in contexts:
    assert (upstream / row['path']).read_bytes() == (W / 'current-source' / row['path']).read_bytes()
for row in read(W / 'target-context-inventory.json'):
    assert (R / row['path']).read_bytes() == (W / 'target-context' / row['path']).read_bytes()
for key in ['050', '051', '013']:
    child = R / v.EVIDENCE / 'receipts' / f'ZS1-{key}.master.json'
    assert child.read_bytes() == (W / f'dependency{key}/receipt.json').read_bytes()
    receipt = read(child)
    for artifact in receipt['artifacts']:
        v.artifact(R, artifact)
append = '''

## Controller independent directory acceptance — 3.1.21

The controller independently read this complete22916-byte candidate including its unchanged historical appendix, and the entire offline verifier before running it once in a fresh copied package. The38 offline checks validate157 payloads, the exact historical59-file payload, relevant current upstream identities and the real050/051/013 receipt chains. They do not replay Node, Vitest, providers, Cargo, PTY or archived scripts. Current scope is12 direct context files and7 direct directories, with zero frozen direct files and only050/051 formal child directories. Each is already independently accepted; this report accepts054 only.

The controller read the complete corrected four-case directory.test.ts, all four actual observations, the initial assertion failure and successful stdout, runner/config/bridge and structural810–900 excerpt. The original test differs only in whether the summary assertion examines JSON.stringify or the actual text field; both exact files and that difference are offline checked. Initial3pass/1fail and corrected4pass are preserved as historical runs. D054-01/02 observe actual summary request prompts, not final summary publication or model fidelity. D054-03 actually aborts while a Promise-gated projector ignores the signal, then both projectors complete in order. This is evidence against unconditional callback cancellation, not a passing cancellation-safety claim. D054-04 rejects the first projector and prevents the second, without proving rollback of any prior external effect. The fixture never instantiates a durable AgentHarness/Session backend; its test Promise gate is not the production effect gate.

The controller newly read complete config.ts, session/context.ts64, runtime/transcript.ts and messages.ts. It also read runtime/harness1–100 and300–425, generation1–245, hooks1–78, execution/assistant115–175, restore1–130, facade505–570 and its final constructor interface/export. These bounded context reads establish actual option storage, config validation, coherent session restoration entry, lane ancestry scanning, ordered projectors, provider transformation, effect-gated hooks/model requests, and close/fault ownership. They do not claim whole-file acceptance of the larger context modules. The051 canonical/013 leaf and051 original master/rebind reviews were read again. Unchanged012 complete-source understanding/37 historical tests and050 lane/structural publication/recovery and bounded Zenpi context/session/core mapping are reused through their exact current accepted chains, not reexecuted or relabeled as new whole-file reads.

Ordinary generation scans current ancestry through the latest compaction, applies custom entryProjectors, then transformContext and toProviderMessages. Structural summary preparation takes raw entries and does not implicitly run that same custom projection pipeline. Failed assistants may therefore be omitted from ordinary context but present in raw summary prompts. Shared references and Context propagation are not deep snapshots, cancellation enforcement or durability. Config validation precedes createAgentHarness's restoration try/catch; callback promises can reject independently. HookRegistry gate admission checks abort at its boundary, which cannot be attributed to arbitrary entryProjectors. Harness.close caches a promise after calling session.close and waits for lane idle callbacks; the inspected implementation does not establish forced cancellation of arbitrary callbacks or independent storage atomicity. Session publication/recovery remains with the durable owner, not the context projection helper.

All relevant upstream identities,050/051/013 receipt artifacts and three bounded Zenpi target identities were rechecked against current main. Later521c headless/92f95 production and the new headless_domain_owner regression do not touch these three target files and are not used as054 evidence. The worker captured31ea/mio and other concurrent progress remains historical. No unrelated checkbox or Gantt change is treated as a source semantic change; the global G-STAGE policy is unchanged.

Accept054 only after050 and051. No context-only direct file, non-frozen child, leaf012/013, parent057/061, target owner or complete103/104/115 is newly accepted here. The documented raw-summary/projected-context distinction, heuristic summary fidelity, cooperative cancellation and storage/recovery limits remain explicit. Rollback withdraws054 report/receipt/status only and preserves children, product sources, user sessions and all historical failures.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['offline.stdout.json', 'offline.stderr.log', 'offline.run.json']:
    shutil.copy2(D / name, Q / name)
dump(Q / 'current-context.json', {row['path']: meta(upstream / row['path']) for row in contexts})
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
     baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='harness054-current-3.1.21',
     integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, scope=scope,
     folder_path=folder, children=list(bp.items[KEY].depends), artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker B with historical A evidence and independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Full054 report, verifier, corrected historical tests/observations and necessary actual caller context read.050/051 separately accepted,12 context files/7 physical directories with only050051 formal children. Preserve raw summary vs projector and cancellation limits; only054 accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-054\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
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

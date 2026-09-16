from pathlib import Path
import datetime, hashlib, json, subprocess, sys

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

Q = R / 'Docs/quality/stage1/ZS1-401/master-review-3.1.21'
assert not (Q / 'manifest.json').exists()

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def read(p):
    return json.loads(p.read_text())

def dump(p, x):
    p.write_text(json.dumps(x, ensure_ascii=False, indent=2) + '\n')

# The prior one-shot completed source review, package verification, receipts,
# [_] validation and [x]/index writes; it stopped at stale401 claim detection.
# Preserve that attempt and finalize only the missing current-ledger checks.
assert read(Q / 'before-promotion.json')['ok']
bp = v.parse((R / v.BLUEPRINT).read_text())
assert bp.items['ZS1-401'].state == bp.items['ZS1-400'].state == '[x]'
sel = v.selector(R, bp, v.BLUEPRINT)
claims = read(R / v.EVIDENCE / 'claims.json')['claims']
assert {c['item_id'] for c in claims} == {'ZS1-402', 'ZS1-017', 'ZS1-084'}
receipt = read(R / v.EVIDENCE / 'receipts/ZS1-401.master.json')
for artifact in receipt['artifacts']:
    v.artifact(R, artifact)
front = v.frontiers(bp, claims, sel['run_id'], 3)
for todo in (R / v.EVIDENCE).glob('todos_*.md'):
    g.atomic(todo, v.todo(bp, front, v.BLUEPRINT, v.EVIDENCE + '/claims.json'))
(Q / 'controller-claim-recovery.md').write_text('''# Controller ledger correction

The initial401 acceptance attempt exited1 in frontiers after writing its accepted checkbox/index, because the controller had not yet moved task A's claim from completed401 to the already-running402. The semantic report, source identities, copied-package offline verifier and [_] validation had passed. This is a controller bookkeeping failure, not a product test failure or new semantic evidence. The original accept.py and worker evidence remain unchanged; the old one-shot was not rerun.

An authoritative wait_threads snapshot confirmed task A active on402, task B active on017 and task C active on pending-input work following completed084 consolidation. Claims were brought into agreement with those existing tasks. This fresh finalizer checks the existing401 receipt/artifacts, reprojects todos and runs401 G-STAGE once. No source, test, budget, old program, canonical report or acceptance scope changes are made.
''')
dump(Q / 'controller-claim-failure.json', dict(exit_code=1, command='python3 .ops/stage1_execution/accept_directory401_321.py',
     error='BlueprintError: accepted item has claim', location='frontiers during state([x])',
     preserved_original_attempt=True, products_executed=0))
post = v.validate(R, item='ZS1-401')
dump(Q / 'after-promotion.json', post)
assert post['ok'], post
argv = ['python3', 'tools/validate_stage1_blueprint.py', '--item', 'ZS1-401']
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.run(argv, cwd=R, capture_output=True, timeout=60)
(Q / 'gstage.stdout.log').write_bytes(proc.stdout)
(Q / 'gstage.stderr.log').write_bytes(proc.stderr)
dump(Q / 'gstage.run.json', dict(argv=argv, cwd=str(R), started_at=started,
     ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=proc.returncode,
     stdout=meta(Q / 'gstage.stdout.log'), stderr=meta(Q / 'gstage.stderr.log')))
assert proc.returncode == 0, proc.stderr
(Q / 'finalize.py').write_bytes(Path(__file__).read_bytes())
dump(Q / 'manifest.json', dict(item='ZS1-401', master_accepted=True,
     artifacts={p.relative_to(Q).as_posix(): meta(p) for p in Q.rglob('*') if p.is_file()}))
print(json.dumps(dict(accepted='ZS1-401', counts=post['counts'], snapshot=post['snapshot_sha256'])))

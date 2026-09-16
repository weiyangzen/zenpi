from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-017'
D = R / '.ops/stage1_execution/source017-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-017/master-review-3.1.21'
assert not Q.exists()

def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, data): p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))

assert meta(W / 'manifest.json')['sha256'] == '158f372c911c046cabb37fc6b6824a60d0e930aaedb4c32d8511443d5cefede7'
assert read(D / 'offline.run.json')['exit_code'] == 0
audit = read(D / 'offline.stdout.json')
assert audit['passed'] and len(audit['checks']) == 44
for row in read(W / 'manifest.json')['files']:
    assert meta(W / row['path']) == {k: row[k] for k in ['bytes', 'sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
f = bp.files[KEY]
report = f.artifact
assert bp.items[KEY].state == '[ ]' and bp.items[KEY].depends == ('ZS1-001',)
assert bp.items['ZS1-001'].state == '[x]'
assert not (R / report).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / 'files' / report
assert meta(candidate)['sha256'] == '769f73a1cd40fbdfdb2eed50c871442cdfe4d15489870a3961adf0be179a4e4b'
current = {}
for row in read(W / 'input-inventory.json'):
    if row['kind'] != 'authority':
        p = Path(row['origin'])
        assert meta(p) == {k: row[k] for k in ['bytes', 'sha256']}
        current[row['origin']] = meta(p)
append = '''

## Controller independent file acceptance — 3.1.21

The controller read the complete 16027-byte, 526-line google-generative-ai.ts source and the complete 18512-byte candidate report. It independently read the complete google-shared.ts453, simple-options.ts95 and event-stream.ts110 support files, current google.rs759, the entire actual historical Bun probe and its17-case/20-HTTP result and command receipt, and the complete current stage1_gemini.rs971 plus its historical17-test output and command receipt. Current19 test definitions include two later terminal-cancel tests; the old17 receipt does not cover those additions. The controller did not execute Bun, Cargo, providers, archived runners, PTY or budget measurements in this review.

The controller additionally read the complete transform-messages.ts context and models.ts880–965, covering actual cost calculation and thinking-level clamping. These refine the candidate's summary: tool-ID normalization is invoked for cross-model/API/provider conversion, not unconditionally for same-model native histories; transformMessages first normalizes null content, handles unsupported images, drops redacted foreign reasoning and strips incompatible signatures. Its second pass skips error/aborted assistant messages and synthesizes error tool results for unresolved calls. The Google helper then applies its own provider/model/base64 checks and wire conversion. Same-model is not defined identically at every layer. No supporting file is independently accepted by017.

The max-token helper limits available context using a floor of1 for available space, but Math.min(maxTokens, available) can still preserve a caller-supplied zero or negative cap. This source layer has no general numeric validation; the candidate's 'at least1' phrasing applies to available room, not an unconditional positive final result. Cost tiers use the highest strictly exceeded input threshold and mutate usage.cost; Google input subtracts cached input while Zenpi's public input includes it. Raw stream and streamSimple have different error timing and parameter normalization. Missing auth in streamSimple throws synchronously, whereas errors inside raw stream become terminal events. onPayload may replace all parameters after abortSignal construction and is not revalidated. Initial SDK construction retry does not cover subsequent iterable failures. Arbitrary mid-stream cancellation, SDK internal retries, abnormal usage, concurrent streams/backpressure and every thinking model family were not exercised by the historical source probe.

The independent target review covered request validation, native parts and integrity comparison, identity consistency, STOP-only completion, ordered deltas, bounded JSON/SSE reads and terminal cancellation guards. Source text/thinking blocks merge adjacent same-kind parts and keep the latest nonempty signature; Zenpi retains individual native parts. The source can emit toolcall_end and later error, and MAX_TOKENS can terminate as length. Zenpi does not publish executable ToolCallDone until Fold.finish passes full completion validation, although earlier deltas can already have been observed. Cancellation during the terminal batch can retain the first already-published tool; the later guard stops subsequent events, not prior events. Native history digests are local integrity checks, not remote signature verification. Plain unsigned text can cross some model boundaries; opaque reasoning history cannot.

The controller read the exact captured backend1036–1050,1209–1320,1587–1702, transport58–103/308–328/411–468, core3855–3882 and registry245–290 excerpts. These bind real request dispatch, x-goog-api-key, private cancellation, finite body polling, no retry after a published event, retry suppression for semantic_compaction, capability defaults and durable assistant-turn publication. The scoped transport reads preserve non-Unix connection and non-macOS resolver limits. They are not whole-file acceptance. The current521c headless and6f121 domain regression identities match the already independently reviewed explicit blueprint version fix; that evidence is reused only to explain current CLI composition, not as Google model-version validation.

The historical Bun server returns prepared SSE through actual @google/genai2.21.0, with no mocked imports. Its17 cases produced20 requests; these are not37 independent tests. The complete target test reader confirms actual local TCP/backend cases and CLI temporary-session restarts. Some negative target assertions check only is_err without an exact error classification, so they do not establish every named pre-network rejection point. In particular the saved-model resume fixture also has incomplete native metadata; its assertion alone does not isolate model mismatch as the exclusive failure cause. The actual production path was independently read rather than inferred from that test name. Local fixtures do not prove live Google service compatibility. Historical source/target outputs remain bound to their original revisions; current source hashes matching a later map do not revalidate the whole original CLI binary.

Before publication, a new copied package's fully read standard-library verifier ran once with --live, output outside the immutable copy:44 checks passed, covering469 payloads, the complete old12/40/86 payload packages and forward/reverse patch data, three report-prefix identities,13 old mapping excerpts, current related source identities,017 scoped authority and5258 protected original files. This is integrity verification, not semantic proof or a runtime test. Original G-STAGE failures from missing017 master receipt and the historical preparation FileNotFound record remain in the archive. The controller's initial optional lookup of a nonexistent016 master-review-3.1.21 path also failed; the actual016 receipt points to master-review-3.1.20 plus rebind3.1.21. No product failure or new016 acceptance is inferred from that lookup.

The original worker report is preserved byte-for-byte as the prefix of this report and separately in the public archive. Only017 receives acceptance after its001 dependency. Unmapped source configuration surfaces and broader model families stay explicit differences, not claims of complete product parity.055 parent, source siblings,110 Gemini product and target files remain separately owned and unaccepted by this action. Rollback withdraws only017 report/receipts/status; all product files, user state, historical evidence and sibling acceptances remain.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(append.lstrip())
shutil.copy2(candidate, Q / 'worker-report.md')
shutil.copy2(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copy2(Path(__file__), Q / 'accept.py')
for name in ['offline.stdout.json', 'offline.stderr.log', 'offline.run.json']:
    shutil.copy2(D / name, Q / name)
extra = {}
upstream = Path(bp.header['source_repo'])
for rel, lo, hi in [('packages/ai/src/api/transform-messages.ts', 1, None), ('packages/ai/src/models.ts', 880, 965)]:
    p = upstream / rel
    dest = Q / 'additional-context' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    lines = p.read_bytes().splitlines(True)
    dest.write_bytes(b''.join(lines[lo-1:hi]))
    extra[rel] = dict(source=meta(p), start_line=lo, end_line=hi or len(lines), excerpt=record(dest))
dump(Q / 'current-inputs.json', dict(captured_inputs=current, additional_context=extra))
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file(): archive.add(p, arcname=p.relative_to(W).as_posix(), recursive=False)
sizes = v.file_hashes(bp, R, upstream, target=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    dest = Q / 'authority-before' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(R / rel, dest)
(R / report).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / report, candidate.read_text() + append)
refs = [record(R / report)] + [record(Q / name) for name in ['review.md', 'worker-report.md', 'worker-manifest.json', 'worker-ready.tar.gz', 'offline.stdout.json', 'offline.run.json', 'current-inputs.json', 'accept.py']]
refs += [record(p) for p in (Q / 'additional-context').rglob('*') if p.is_file()]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'], requirement_digest=bp.requirement,
    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'], complete=True, attempt_id='source017-current-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators, source_path=f.path,
    source_hash=f.sha256, read_ranges=[[0,6110],[6110,11521],[11521,16027]], artifacts=refs)
for role in ['worker', 'master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker B with frozen C evidence and independent controller qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=record(Q / 'review.md'),
            findings='Entire017 source, candidate, actual historical probe and current Google target/test context read.44 scoped integrity checks pass; old17 cases/20HTTP and17 tests remain historical.Qualify cross-model ID normalization, token floor, error-test limits and source-target differences.Only017 accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt, ensure_ascii=False, separators=(',', ':')) + '\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-017\*\*)', '- ' + mark + r'\1', (R / v.BLUEPRINT).read_text(), flags=re.M)
    assert count == 1
    updated = v.parse(text)
    assert updated.requirement == bp.requirement
    g.atomic(R / v.BLUEPRINT, text)
    active = read(R / v.SELECTOR)
    active['snapshot_sha256'] = updated.snapshot
    g.atomic(R / v.SELECTOR, json.dumps(active, ensure_ascii=False, separators=(',', ':')) + '\n')
    for rel, (fields, rows) in v.scaffold(updated, sizes, v.EVIDENCE).items(): g.atomic(R / rel, v.tsv(fields, rows).decode())
    front = v.frontiers(updated, read(R / v.EVIDENCE / 'claims.json')['claims'], sel['run_id'], 3)
    for todo in (R / v.EVIDENCE).glob('todos_*.md'): g.atomic(todo, v.todo(updated, front, v.BLUEPRINT, v.EVIDENCE + '/claims.json'))

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
g.main()
print(json.dumps(dict(accepted=KEY, counts=post['counts'], snapshot=post['snapshot_sha256'])))

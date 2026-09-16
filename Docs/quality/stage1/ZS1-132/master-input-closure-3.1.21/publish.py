from pathlib import Path
import hashlib, json, re, shutil, tarfile
R = Path(__file__).resolve().parents[2]
D = R / '.ops/stage1_execution/input-shutdown-current-3.1.21'
I = D / 'root-integration'
V = R / '.ops/stage1_execution/input-shutdown-release-3.1.21'
Q = R / 'Docs/quality/stage1/ZS1-132/master-input-closure-3.1.21'
assert not Q.exists()
def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def save(p, x): p.write_text(json.dumps(x, ensure_ascii=False, indent=2) + '\n')
verification = read(V / 'master-verification.json')
assert read(I / 'verification.json')['status'] == 'passed'
assert all(meta(R / p) == x for p, x in read(V / 'inputs.json').items())
assert meta(R / 'target/release/zenpi') == verification['current_binary']
assert verification['current_binary']['sha256'] == 'd0ccf6f5d13dc0e04abe489335bc66be7cee3d0014501b4eb42bc9514dc3c05f'
assert not verification['budget_ok'] and [k for k, ok in verification['gates'].items() if not ok] == ['cold_start']
assert read(V / 'budget.run.json')['exit_code'] == 1
assert read(D / 'root-before.run.json')['exit_code'] == 101
assert 'test result: FAILED. 2 passed; 1 failed;' in (D / 'root-before.log').read_text()
counts = [int(n) for name in ['regression', 'related'] for n in re.findall(r'test result: ok\. (\d+) passed; 0 failed', (I / (name + '.log')).read_text())]
assert counts == [3, 7, 13, 43, 6, 23] and sum(counts) == 95
assert '0 passed; 0 failed' in (I / 'tickets.log').read_text()
for name in ['regression', 'related', 'clippy', 'format']:
    assert read(I / (name + '.run.json'))['exit_code'] == 0
for case, count in [('projects', 12), ('bentobox', 11)]:
    result = read(V / case / 'manifest.json')
    assert result['status'] == 'passed' and result['cases'][case]['check_count'] == count
    assert result['binary_sha256'] == verification['current_binary']['sha256']
review = '''# Controller review: close admitted input tickets before shutdown acknowledgement

An input admitted while a finite provider read is busy could remain reserved without a terminal response after explicit shutdown. The controller reproduced the failure on main521c with the new durable regression: two controls pass and the delayed two-ticket case fails specifically for missing terminal responses, not the shutdown deadline. The product now closes pending input responses on RuntimeEvent::Closed before emitting the shutdown acknowledgement. It cancels tickets not yet serviced, preserves any available receipt, and otherwise records input_queue_result_unknown with explicit advice that the input may already have been applied and the queue must be inspected before retrying. Original schema version, project/session correlation and durable cached replay are preserved.

The controller independently reviewed the complete13690-byte candidate patch, all327 new test lines, public embedding fixture and wire probe, close/event/input-drain/replay code and bounded InputPort/owner lifecycle contexts. Final frozen patch, test and probe match the preliminary bytes exactly. The copied sealed package's verifier was fully read and run once from /tmp with external logs. It checks3218 payloads,224 original build inputs, exact read ranges and binary identities, and temporary-git application/duplicate rejection/rollback. All retained failure evidence remains in the archive. This is review of the29-line headless delta, not a new complete reading or acceptance of9334-line084.

The controller had already installed the precise new test before applying the product patch. Its new product-only patch therefore changes only src/headless.rs, without deleting/recreating the existing test or forcing the full patch. Baseline headless521c2102519b502fbd6184f31ffb53cba8eeef840bf84d19450836429d9b49f0 becomes bf252fec95a859b26f9aea402c918e9efbf6c2c55d3e24b403831f86d80ec06e. The test is10856 bytes/327 lines,5e4c55fe667930ecef2229f19b0897c4a4c50e42012301d768118d44d20969f5. The225 captured inputs stayed identical through testing, release build and production checks. No raw-mode, parser, TUI or BentoBox code changed in this integration.

Root validation passes95 tests:3 new shutdown-input regressions,7 domain owner,13 project workspace,43 headless protocol,6 resume/compact owner and23 input-host tests. The input-host suite includes the three existing ticket tests; its2 pre-existing ignored tests stay ignored. An extra lib input_queue:: filter matched zero tests and is retained as an empty invocation, not counted or used as evidence. All-target Clippy with warnings denied and fmt check pass. The same root regression test ran with fixed test-threads=1 before and after, preserving the2-second acknowledgement deadline; total test durations2.27s before and2.34s after are whole-suite time, not per-ack latency.

Worker run_stdio_owned process evidence remains distinct from the root run_async_streams integration tests: a corrected finite five-case/ten-host baseline records85/100 checks and fifteen failures caused by three missing ticket terminals; candidate records100/100 with every host naturally exiting0. Ready and EOF controls intentionally mutate the queue. The deliberately unserviced cases do not. The controller independently reconstructed152 raw wire/WAL/journal and post-reconnect facts from saved runs; the worker separately reconstructs272 static facts. Neither count is an additional runtime test count. Both inspect preserved prefixes, exact ticket terminal replay, original identity and no new ticket or queue append during reconnect. The root did not rerun the frozen external probe.

Unknown means outcome unknown. Cancellation cannot undo an already-serviced or in-service queue mutation. No fixture deliberately pauses within an in-service transaction and no cross-project switch race is proved. Ordinary EOF draining and the existing250ms/1s shutdown grace boundaries are unchanged. Permanently uncooperative owners, write failures and unexpected disconnection without Closed are not established as fixed. The first external probe omitted limit and timed out; initial unformatted test identity, formatted baseline default-parallel deadline failures and the read-block preparation assertion failure remain preserved. The later fixed serial diagnostic is separately identified rather than replacing those failures.

The new main production release is6073504 bytes, SHA256 d0ccf6f5d13dc0e04abe489335bc66be7cee3d0014501b4eb42bc9514dc3c05f. The unchanged budget harness built it and measured exactly its first three launches:1066.501958/46.088916/47.311125ms. Cold start FAILS the original1000ms maximum; the other7 gates pass. First sample exceeded the cap by66.501958ms. All three actual processes exit0 and emit a drained shutdown acknowledgement, but functional success does not make the timing gate pass. The retained parent observation has2.2825ms Popen return and1063.848167ms communicate; it does not attribute internal product phases or prove the cause. No prewarming, retry, discarded sample, no-fail mode or threshold/timing change occurred. Historical92f95's8 passing gates remain historical, not the status of this new binary. Worker1aeae2's build and6f9b6e embedding are distinct binaries.

After the budget measurement, this same immutable new release passed two official real-PTY regressions in new output directories:12 project checks and11 BentoBox checks. The project case clicks the top-row plus to open the folder picker, verifies Esc/invalid paths create no placeholder, binds Unicode/spaced paths and project models, executes real shell writes in the selected folders, retains late output in its original project and restores selected project and draft after restart. The BentoBox case exercises keyboard/mouse layout, collapse/focus, resize, persistence and recovery. These23 checks support those concrete interactions; they are not full TUI parity or an execution of the whole Stage1 host matrix. The release has not rerun the previous explicit-version35-request CLI matrix or full kill/yank suite, which retain their earlier binary identities.

This product delta is integrated with functional regression evidence, while117 cold-start remains an explicit failing gate.084/132/117 and the whole blueprint remain unaccepted. The earlier406 directory review captured69 test files; adding this fully read and tested327-line file makes70 current direct test files and is a separately recorded scope delta, not proof that the old inventory contained it. Complete per-file/per-dir understanding and all remaining product boundaries still require their own acceptance. Rollback of this delta uses the original inverse product patch and removes only the newly introduced test after preserving evidence; it must preserve unrelated dirty changes, every original failure and user state.
'''
Q.mkdir(parents=True)
(Q / 'review.md').write_text(review)
(V / 'review.md').write_text(review)
for name in ['master-verification.json', 'budget.json', 'budget.run.json', 'binary.json', 'inputs.json']:
    shutil.copy2(V / name, Q / name)
for name in ['root-before.log', 'root-before.run.json', 'root-raw-review.json', 'sealed-worker-verify.log', 'sealed-worker-verify.run.json']:
    shutil.copy2(D / name, Q / name)
shutil.copytree(I, Q / 'root-integration')
shutil.copy2(R / 'src/headless.rs', Q / 'headless.rs')
shutil.copy2(R / 'tests/headless_input_shutdown.rs', Q / 'headless_input_shutdown.rs')
for name in ['prepare_input_shutdown_root321.py', 'inspect_input_shutdown_raw321.py', 'integrate_input_shutdown_root321.py', 'validate_input_shutdown_release321.py']:
    shutil.copy2(R / '.ops/stage1_execution' / name, Q / name)
shutil.copy2(Path(__file__), Q / 'publish.py')
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted((D / 'worker').rglob('*')):
        if p.is_file(): archive.add(p, arcname=p.relative_to(D / 'worker').as_posix(), recursive=False)
with tarfile.open(Q / 'production-evidence.tar.gz', 'x:gz') as archive:
    for p in sorted(V.rglob('*')):
        if p.is_file(): archive.add(p, arcname=p.relative_to(V).as_posix(), recursive=False)
save(Q / 'manifest.json', dict(complete=False, product_delta_integrated=True, release_budget_passed=False, artifacts={p.relative_to(Q).as_posix():meta(p) for p in Q.rglob('*') if p.is_file()}))
print(json.dumps(dict(public=str(Q), manifest=meta(Q / 'manifest.json'), rust_passed=95, pty_checks=23, cold_start_passed=False)))

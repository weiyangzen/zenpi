from pathlib import Path
import datetime,hashlib,json,re,shutil,sys,tarfile
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/shutdown-current-3.1.21';H=D/'release-hosts';P=R/'Docs/quality/stage1/ZS1-132/master-shutdown-release-3.1.21';assert not P.exists()
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def read(p):return json.loads(p.read_text())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
after=read(D/'inputs-after.json');assert all(meta(R/p)==m for p,m in after.items())
for n in ['worker-offline','apply-check','apply','root-tests','root-clippy','root-fmt','embedding-build','probe-after']:assert read(D/(n+'.run.json'))['exit_code']==0,n
assert read(D/'probe-before.run.json')['exit_code']==1
for n in ['budget','project-plus','kill-yank']:assert read(H/(n+'.run.json'))['exit_code']==0,n
budget=read(H/'budget.json');assert budget['ok'] and len(budget['gates'])==8 and all(budget['gates'].values());binary=meta(H/'zenpi-release');assert binary==meta(R/'target/release/zenpi') and budget['cold_start']['binary_sha256']==binary['sha256']
project=read(H/'project-plus.json');edit=read(H/'kill-yank.json');assert project['status']=='passed' and len(project['assertions'])==12 and project['binary_sha256']==binary['sha256'];assert edit['passed'] and len(edit['checks'])==29 and all(edit['checks'].values()) and edit['request_count']==4 and edit['tui_processes']==3 and edit['binary_sha256']==binary['sha256']
assert [int(n) for n in re.findall(r'test result: ok\. (\d+) passed; 0 failed',(D/'root-tests.log').read_text())]==[13,43,12]
raw=read(D/'controller-raw-probe-verification.json');assert len(raw['before']['failed'])==4 and not raw['after']['failed']
verification=dict(complete=False,top_level_rust_tests=68,real_embedding_checks=53,original_embedding_failures=4,embedding_binary=read(D/'embedding-binary.json'),production_release=binary,budget_gates=budget['gates'],cold_samples_ms=budget['cold_start']['elapsed_ms']['samples'],project_PTY_assertions=12,kill_yank_PTY_checks=29,kill_yank_HTTP_requests=4,kill_yank_TUI_processes=3,current_inputs=after)
dump(D/'master-verification.json',verification)
review='''# Controller: bounded owned-host cleanup and current release — 3.1.21

Current main headless944049→31ea4fcc removes a demonstrated post-shutdown Agent mutex wait, with mio41321e, project tests8375, TUI and BentoBox bytes preserved. The sole product change in this integration is headless.rs. The controller independently read the entire candidate patch and review, complete63-line public embedding host and105-line probe, complete portable verifier, and actual headless replay/cleanup, runtime shutdown/detach, Agent try_close and live-owner epoch boundaries. This is a bounded delta review, not whole084 acceptance.

After the runtime has joined its scheduler and detached a non-cooperative job, transport replay ownership is released without acquiring the Agent lock. Captured root/pool Arcs are deduplicated. Available owners unregister the captured epoch and close immediately; locked owners go to one named cleanup thread per host. The thread uses captured owners rather than resolving a later project owner. Immediate errors/spawn failure propagate; deferred errors are visible on stderr; a transport error retains priority with a secondary cleanup diagnostic. ReplayState::close currently only takes/drops its journal and returns Ok: an initial controller concern about its hypothetical returned error skipping cleanup is not a reachable current defect and was explicitly corrected to the worker. No grace, queue, fd, backend, session or owner module was changed.

The private worker package2259 payloads and its prior exact944049 provenance chain pass independent portable verification in a fresh copied package; this verifier does not execute archived providers/builds. The controller current main passes68 top-level tests (13 project,43 protocol,12 runtime), Clippy with warnings denied and fmt. The old Esc integration separately passed144 root tests on944049+41321e; that earlier144 is not rebranded as this31ea runtime execution.

The current embedding binary was built in its own new source/target directory. All Rust/Cargo/vendor inputs match current main. One non-build document generator, tools/generate_stage1_gantt.py, was captured during a controller draft update; this exact difference is recorded in embedding-input-drift.json and both manifests, not silently normalized. Main restored its captured generator until validation completed, with the draft saved separately for the final Gantt refresh. No runtime result was rerun because of this unrelated document difference.

The exact same bounded public-API probe executes in two fresh root-owned output directories: preserved worker baseline embedding (headless944049, its historical other inputs) has4 failures among53; current31ea+41321 embedding passes53/53. Both use real Agent, public Backend/ProviderEvent, actual loaded close hook, JSONL/WAL/journal and10 processes, all reaped with reader threads joined. This is generic finite non-cooperative-provider behavior, not a claim about an HTTP provider. Before API return after shutdown takes5.001/5.005/4.992s; current takes1.282/1.278/1.281s. Control receipt time is separate from API return and whole-process exit. Ordinary busy EOF still drains its1.8s job (current API elapsed1.928s); it is not converted into a shutdown timeout.

Controller raw-file checks independently confirm a single cancelled work terminal in stdout and WAL with original project identity, terminal preceding shutdown ack, no events after that ack, and no late assistant journal turn. Two lingering embeddings close their captured owner once after the job returns and persist one cancelled operation. Immediate process exit can truncate the detached job and cleanup: zero old-host hooks and an unfinished operation are preserved, not falsely reported as cleanup success. Reconnect occurs after process reap and replays the exact cached terminal/events; it does not prove safety for concurrent new journal writers while the old job remains alive. Permanent non-cooperation, blocking close hooks/filesystems, thread-spawn/close fault injection and all lifecycle permutations remain unproven. At most one deferred cleanup thread per host does not make arbitrary Rust code killable.

A new production release containing31ea+41321e was built by the unchanged budget harness and first launched as sample0. All8 original gates pass with exactly3 samples, no prewarming, no-fail, widened threshold or retry. This default budget run does not newly establish the optional near-cap journal scenario. The prior first-sample failures remain historical evidence. Actual size/hash/times are in master-verification.json and release-hosts/budget.json.

This exact production binary then passes the full project workspace PTY12 assertions: top-row mouse plus opens the folder picker directly; Esc/invalid path leaves no placeholder; Unicode/space paths and aliases bind the actual selected cwd; equal basenames are distinct; project-specific backend model is shown; real approved shell writes stay in the chosen folders; background output remains with its source project; restart preserves project and draft; slash selection uses the same owner; terminal restoration succeeds. Fixture auth avoids user credentials, HOME/CODEX_HOME are inherited, and all writes target temporary fixtures. The successful project fixture is cleaned by the existing harness; its report/log and binary are durable, not a retained full successful PTY byte stream. No provider operation is submitted by these shell scenarios. The same release passes29 kill/yank checks across3 TUI processes and4 real loopback HTTP requests, retaining the original edit/readiness behavior. BentoBox code is unchanged.

These results establish the two scoped fixes and current user-critical entrypoint checks. They do not accept all120/129/131/132/117, all084, directories or the whole stage.099 and050 were separately accepted with their own reports and receipts. Remaining blueprint requirements and historical failures stay open.
'''
(D/'review.md').write_text(review);P.mkdir(parents=True)
for n in ['review.md','master-verification.json','inputs-before.json','inputs-after.json','embedding-inputs.json','embedding-input-drift.json','embedding-binary.json','controller-raw-probe-verification.json','controller-gantt-draft.json','root-tests.log','root-tests.run.json','root-clippy.run.json','root-fmt.run.json']:shutil.copy2(D/n,P/n)
for name in ['integrate_shutdown_321_current.py','release_shutdown_escape_321.py']:shutil.copy2(R/'.ops/stage1_execution'/name,P/name)
shutil.copy2(Path(__file__),P/'publish.py')
shutil.copytree(H,P/'release-hosts')
with tarfile.open(P/'integration-evidence.tar.gz','x:gz') as t:
 for p in sorted(D.rglob('*')):
  rel=p.relative_to(D)
  if rel.parts[0] not in ['embedding-target'] and p.is_file():t.add(p,arcname=str(rel),recursive=False)
dump(P/'manifest.json',dict(complete=False,artifacts={str(p.relative_to(P)):meta(p) for p in P.rglob('*') if p.is_file()}))
print(json.dumps(dict(public=str(P),manifest=meta(P/'manifest.json'),binary=binary,budget=budget['ok'],cold_samples_ms=verification['cold_samples_ms'],project_PTY=12,edit_PTY=29)))

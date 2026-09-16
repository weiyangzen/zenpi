# -*- coding: utf-8 -*-
"""One-shot publication of verified current integration, with original failures."""
from pathlib import Path
import hashlib,json,re,shutil,sys,tarfile
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/readiness-identity-3.1.21';H=D/'hosts'
P=R/'Docs/quality/stage1/ZS1-129/master-readiness-identity-3.1.21';P.mkdir(parents=True,exist_ok=False)
sys.path.insert(0,str(R/'tools'));import validate_stage1_blueprint as v
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def read(p):return json.loads(p.read_text())
after=read(D/'inputs-after.json');assert all(meta(R/p)==m for p,m in after.items())
counts=list(map(int,re.findall(r'test result: ok\. (\d+) passed', (D/'tests.log').read_text())))
assert counts==[12,43,7,19,16,46] and sum(counts)==143
for n in ['worker-A-offline','worker-C-offline','worker-A-apply-check','worker-C-apply-check','worker-A-apply','worker-C-apply','tests','clippy','fmt','release']:
 assert read(D/(n+'.run.json'))['exit_code']==0
budget=read(H/'budget.json');assert budget['ok'] and all(budget['gates'].values()) and len(budget['cold_start']['elapsed_ms']['samples'])==3
binary=read(D/'binary.json');assert meta(D/'zenpi-release')==binary and budget['cold_start']['binary_sha256']==binary['sha256']
for name,code in [('budget',0),('pty-before',1),('pty-after',0),('jsonl-before',1),('jsonl-after',0)]:assert read(H/(name+'.run.json'))['exit_code']==code
pty=read(H/'pty-after.json');assert pty['passed'] and len(pty['checks'])==29 and pty['request_count']==4 and pty['tui_processes']==3
oldpty=read(H/'pty-before.json');assert not oldpty['passed'] and len(oldpty['checks'])==11 and 'condition timed out' in oldpty['error']
for name,fails in [('jsonl-before',12),('jsonl-after',0)]:
 d=read(H/name/'result.json');assert 'exception' not in d and len(d['checks'])==35 and sum(not v for v in d['checks'].values())==fails
 assert d['host_exit']==0 and d['host_reaped'] and d['reader_threads_joined'] and d['server_thread_joined']
checked=v.validate(R);assert checked['ok']
result=dict(complete=False,items=['ZS1-129','ZS1-131','ZS1-132','ZS1-117'],rust_tests=143,pty_checks=29,jsonl_checks=35,budget_gates=budget['gates'],cold_samples_ms=budget['cold_start']['elapsed_ms']['samples'],binary=binary,requirement_digest=v.parse((R/v.BLUEPRINT).read_text()).requirement,remaining='Per-file/directory acceptance and full production/UX matrix still required; historical failures preserved',validation=checked)
(D/'master-verification.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
review='''# Readiness / asynchronous response identity — current integration

Two real defects are fixed in current main: terminal polling retains readiness and residual TTY bytes across a returned event; asynchronous QueueFull and Resume summaries now carry the project identity captured for that request. The controller independently reviewed both full patches, their complete reports and offline verifiers, the complete477-line final mio source (production1–285 plus all appended tests in the full patch), current headless request-context helper and both affected control branches. Existing unchanged source-reading chains are reused by exact hashes. This is not new whole-headless/TUI or directory acceptance. Canonical parser, raw-mode/fd flags, TUI994215 and BentoBox are unchanged.

Registration3.1.21 happened before product application. New099/133 and directory400–406 remain unaccepted. The228? count is not used: the actual captured build inventory contains224 files; exactly three change here: headless.rs, vendor mio.rs and the appended composer regression. Headless2f083→0ece91 preserves the previous busy/diff repair; mio57f4→1afaaa preserves CRLF and the existing reset API. Composer old1202-line content remains an exact prefix with one49-line regression appended. All other captured inputs remained byte-identical through validation. Both sealed worker packets passed portable verification and temporary-Git apply/rollback checks. Current main patches were separately checked before application; original input bytes, patches and all raw failures remain archived.

Semantics: pending tokens persist until consumed; parser output retains priority; FIONREAD avoids a speculative continued blocking stdin read while leaving UTF8/CSI/paste state intact. Empty residual/WouldBlock retire the TTY token; EOF/read errors return errors. Only the known three token kinds are registered. The added native composer test checks logical-line deletion/yank across140×40→1×1→140×40 with Unicode and following-fold identity. Kernel single-edge, blocking and event-stream test evidence from worker4/5 cases is preserved, but it is not described as a new controller run or cross-platform proof. Ordinary bounded tests do not establish arbitrary EINTR-flood or unclosed-paste bounds.099 independently records residual parser limitations.

For headless, QueueFull now attaches the admitted project before releasing its reservation and still remains retryable. Resume only decorates its summary before stdout; its historical event suffix, write-before-release order and repeat replay stay unchanged. request_projects takes precedence over emission_project. No global cwd, queue limit, cancellation or path guard changed.

Controller actual current runs: six native targets pass143 tests (headless workspace12, protocol43, layout7, slash19, BentoBox16, composer46), all-target Clippy and cargo fmt pass. Release build produces6072912B SHA93f3488855c26e3ebb4f9d30a69e4372f3d01070766976ee727d9b7ab4f50fd7. Original dependency unused_parens warning remains. Before any launch of this new release, the unchanged fixed3-sample benchmark runs once against target/release/zenpi:460.119959 /36.881584 /37.090333ms and all8 gates pass. No warmup, threshold/sample/timing-boundary change, no-fail flag or retry. This new revision passing does not erase historical1109ms failure or prove the startup root cause; the separate five-phase report is diagnostic only.

Then the unchanged original kill-yank harness is run with identical child-only proxy isolation on original e9ba and this new release. Original fails after11 checks at the same logical Ctrl-U wait; new passes29 checks,4 HTTP requests,3 TUI processes. Exact Unicode/fold payload,1×1 resize,12-second assertion, project buffers, approval denial, restart and256KiB/4MiB rejection/retry remain intact. No additional wake key or reset was added. Raw .pty.log/.http.json, snapshots and all failure text are archived.

The actual JSONL fixture uses two independent real Git projects, one gated provider and32 pending jobs. Original2f083 binary fails12 project-envelope assertions among35 checks; this new combined release passes35/35. Request release/retry, cached original-project terminal, repeated Resume suffix, v1, foreign guard, project switch/new-session identity and shutdown are exercised. Both hosts exit0; reader/provider threads join, each exactly1 HTTP request. Worker initial fixture KeyError remains in its sealed archive. These raw results support the two changed paths, not every protocol path or full132.

The physical code is integrated and these checks pass. Whole129/131/132/117, full per-file/dir reviews, full current UX production matrix and broader stage closure remain outstanding. Newly observed synchronous shell-event omission is a separate candidate under investigation, not fixed by this increment. Final acceptance remains28/121.
'''
review=review.replace('The228? count is not used: the actual captured build inventory contains224 files','The actual captured build inventory contains224 files')
(D/'review.md').write_text(review)
for name in ['master-verification.json','review.md','binary.json','inputs-before.json','inputs-after.json','source-drift.json']:
 shutil.copy2(D/name,P/name)
for name in ['integrate_readiness_identity_321.py','verify_readiness_identity_hosts_321.py','publish_readiness_identity_321.py']:
 shutil.copy2(R/'.ops/stage1_execution'/name,P/name)
with tarfile.open(P/'integration-evidence.tar.gz','x:gz') as tar:
 for p in sorted(D.rglob('*')):
  if p.is_file() or p.is_symlink():tar.add(p,arcname=str(p.relative_to(D)),recursive=False)
artifacts={str(p.relative_to(P)):meta(p) for p in P.iterdir() if p.is_file()}
(P/'manifest.json').write_text(json.dumps(dict(artifacts=artifacts,complete=False),indent=2)+'\n')
print(json.dumps(dict(public=str(P),artifacts=len(artifacts),binary=binary,counts=counts)))

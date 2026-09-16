# -*- coding: utf-8 -*-
"""Publish a verified integration, then independently accept file133 only."""
from pathlib import Path
import datetime,hashlib,json,re,shutil,subprocess,sys,tarfile
R=Path(__file__).resolve().parents[2];D=R/'.ops/stage1_execution/busy-test-shell-3.1.21'
sys.path.insert(0,str(R/'tools'));import validate_stage1_blueprint as v
from generate_stage1_gantt import atomic
P=R/'Docs/quality/stage1/ZS1-132/master-busy-test-shell-3.1.21'
Q=R/'Docs/quality/stage1/ZS1-133/master-review-3.1.21'
assert not P.exists() and not Q.exists()
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def read(p):return json.loads(p.read_text())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def record(p):return dict(path=str(p.relative_to(R)),**meta(p))
after=read(D/'inputs-after.json');assert all(meta(R/p)==m for p,m in after.items())
for name in ['worker-B-offline','worker-C-offline','worker-B-apply-check','worker-C-apply-check','worker-B-apply','worker-C-apply','root-tests','root-clippy','root-fmt','embedding-build']:
 assert read(D/(name+'.run.json'))['exit_code']==0,name
log=(D/'root-tests.log').read_text()
assert '13 passed; 0 failed' in log and '43 passed; 0 failed' in log
rows=[json.loads(x[len('busy-diff recv '):]) for x in log.splitlines() if x.startswith('busy-diff recv ')]
diffs={}
for row in rows:
 if row.get('type')=='response' and row.get('id') in ['diff-0','diff-1','diff-back','diff-2']:diffs.setdefault(row['id'],row)
assert list(diffs)==['diff-0','diff-1','diff-back','diff-2']
assert all('UNIQUE_DIFF_B' in x['data']['diff'] and 'UNIQUE_DIFF_A' not in x['data']['diff'] for x in diffs.values())
assert len(re.findall(r'^busy-diff http [012] ',log,re.M))==3 and 'busy-diff cleanup host joined; provider joined' in log and 'reaped=true' in log
for name,code,failures in [('shell-before',1,6),('shell-after',0,0)]:
 assert read(D/(name+'.run.json'))['exit_code']==code
 r=read(D/name/'result.json');assert 'exception' not in r and len(r['checks'])==52 and sum(not x for x in r['checks'].values())==failures
 assert len(r['hosts'])==5 and r['all_shell_pids_gone'] and all(h['reaped'] and h['reader_threads_joined'] and h['exit_code']==0 for h in r['hosts'])
checks=dict(complete=False,top_level_rust_tests=56,nested_child_test_is_not_added_to_count=True,shell_before_failed_checks=6,shell_after_passed_checks=52,embedding_binary=read(D/'embedding-binary.json'),production_release_rebuilt=False,budget_rerun=False,current_inputs=after)
dump(D/'master-verification.json',checks)
review='''# Busy /diff durable regression and synchronous shell events

The controller read the full404-line test patch, every original583-line test/helper in contiguous1–95/96–335/336–583 ranges, the entire independent133 report, both portable verifiers, the complete synchronous shell patch/report, the thin shell host and complete108-line actual probe. Current synchronous dispatch, write_events/write_buffered_events and core shell execution boundaries were reviewed as context; this is not full084/core/TUI acceptance.

Current main now includes the persistent busy /diff regression and the synchronous UserShell success drain. Exactly two captured inputs change: tests/headless_project_workspace.rs6e87→8375 (987lines) and src/headless.rs0ece91→944049 (9249lines). Existing original12tests and all Wire/agent helper bytes remain unchanged around the insertion; the original whole file is not claimed to be a prefix. Existing mio1afaaa/composer/TUI and all other224 captured inputs are preserved. Both frozen worker packets pass independent portable hash/source/receipt validation. Main patches were checked before separate application; prior artifacts and all negative runs remain archived.

The new regression uses actual library asynchronous JSONL, two real Git workspaces, a child-only cwd/env and three explicit provider gates. Provider first-delta receipt and withheld terminal establish that four B diff Values were captured while busy; content assertions are deferred only until cleanup, not recomputed after the gate opens. Root current logs contain all four B-only results, path/symlink/foreign guards, /new session change, A/B cache replay, cancellation/late identity and host/provider joined marker. Original twelve tests still cover independent host restart, journals, permissions and protocol distinctions. An outer45-second child deadline and group kill/reap guard this new scenario. Original Wire Drop/join and permission-cleanup limitations are preserved and do not become guarantees. Success TempDir data are cleaned; raw test stdio retains JSONL/HTTP, while worker negative fixtures preserve their directories. No claim of full success-fixture persistence or RSS measurement.

A dedicated empty root-target built the current main: all13 project tests and43 protocol tests pass. The self-spawn child reports one nested test; it is already part of the13 and not counted twice. All-target Clippy and root cargo fmt pass. A different empty embedding-target, with224 exact current source inputs plus one private shell_host example, built the current embedding executable. Neither target is shared across source roots, avoiding the worker's unproven shared-cache anomaly; its actual failed run/binary/depfiles remain preserved, not explained away or rerun for green.

Actual shell before/after comparison uses public run_stdio/run_stdio_owned, real Agent and local shell; Echo is only for the later prompt exposing delayed events. Original0ece91 host has6 failures among52: approved typed user_shell and command! each omit events before terminal, omit immediate Resume events, then emit them under the later prompt. Both normalize to UserShell; slash! is not a correct independent control. Current944049 host passes52/52. Each complete run uses5 real JSONL hosts, all exit0/reaped/readers joined,4 asynchronous shell PIDs absent. Explicit allow/deny, real asynchronous approvals, shell cancellation, A/B selection, cached terminal and event replay remain verified. Durable journal already recorded shell success before the fix; only delivery/correlation was wrong. Synchronous project envelope and cancellation capability are not expanded. This is real public embedding-interface validation, not a production CLI/PTY or provider transport claim.

The shell success branch now drains through existing write_events before writing/caching terminal. Request ID and turn ID associate events correctly; immediate replay includes them and later prompts do not inherit them. Error handling and asynchronous common drain remain unchanged. Neither threshold, queue/path/replay budget, approval policy nor process-global cwd/env changed.

This publication establishes the limited integration and its current checks. Production release93f348 and its8 passing budgets belong to the prior revision; current944049 production release, full UX matrix and whole132/117 remain pending. Old failures are retained.133 receives separate file-understanding review below; this report itself accepts no product item or directory.
'''
(D/'review.md').write_text(review)
P.mkdir(parents=True)
for name in ['review.md','master-verification.json','inputs-before.json','inputs-after.json','embedding-inputs.json','embedding-binary.json','root-tests.log','root-tests.run.json','root-clippy.run.json','root-fmt.run.json','shell-before.run.json','shell-after.run.json']:
 shutil.copy2(D/name,P/name)
shutil.copy2(Path(__file__),P/Path(__file__).name)
shutil.copy2(R/'.ops/stage1_execution/integrate_busy_test_shell_321.py',P/'integrate.py')
with tarfile.open(P/'integration-evidence.tar.gz','x:gz') as tar:
 for p in sorted(D.rglob('*')):
  rel=p.relative_to(D)
  if rel.parts[0] in ['root-target','embedding-target']:continue
  if p.is_file() or p.is_symlink():tar.add(p,arcname=str(rel),recursive=False)
dump(P/'manifest.json',dict(complete=False,artifacts={p.name:meta(p) for p in P.iterdir() if p.is_file()}))

KEY='ZS1-133';bp=v.parse((R/v.BLUEPRINT).read_text());sel=v.selector(R,bp,v.BLUEPRINT);file=bp.files[KEY]
assert bp.items[KEY].state=='[ ]' and all(bp.items[k].state=='[x]' for k in bp.items[KEY].depends)
assert not (R/file.artifact).exists()
for role in ['worker','master']:assert not (R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json').exists()
before=(D/'before/tests/headless_project_workspace.rs').read_bytes()
assert len(before)==23349 and hashlib.sha256(before).hexdigest()==file.sha256
current=(R/file.path).read_bytes();assert hashlib.sha256(current).hexdigest()=='8375f8bab5042cd3a49a177fb5d933d03eb3eb3a615d40c22be45c28f023d313'
worker_report=D/'worker-B/files'/file.artifact
assert meta(worker_report)['sha256']=='a4aa8813d781394d36cd230c4222cfd869433ffbaf39b885e7ae60e098707ff9'
Q.mkdir(parents=True)
append='''

## Controller independent file review and current integration — 3.1.21

The controller independently read the complete original583 lines as contiguous1–95,96–335,336–583 and the full404-line inserted candidate plus this entire17951-byte worker report. Every original helper and twelve tests and all five added functions are covered. Exact apply/reverse source identities and worker byte-reading reconstruction were independently verified; neither a manifest nor a test count replaces this semantic review.

The original current/private qualifications above remain historical worker statements. Main now contains the exact987-line8375 candidate and944049 headless plus1afaaa mio. Root fresh source-specific targets passed13 project/43 protocol tests, all-target Clippy/fmt, and an independent public embedding shell52-case comparison. The nested test-child count is included in13, not a14th test. Root logs independently show four B-content observations while explicit gates remain closed,3HTTP requests, cancellation/guards/new/cache/identity and joined/reaped cleanup. The worker's original12-pass/new1-fail baseline and shared-target anomaly remain immutable; no Cargo root-cause proof is claimed.

Scope decision: accept only133 complete file understanding of frozen6e87 baseline and current8375 test file, with all limitations in this report. Tests using Echo establish control/journal behavior only; the new busy case uses real loopback backend without claiming semantic model output. Direct TUI host models are not PTY. Permission tests assume a non-root Unix account. Original helpers lack some total failure bounds; new outer deadline is not proof for arbitrary escaped processes or every timeout branch. No406/tests directory,091root,084headless,123/132product,117release or full stage is accepted. Their current remaining obligations persist.
'''
(Q/'review.md').write_text(append.lstrip())
shutil.copy2(worker_report,Q/'worker-report.md')
shutil.copy2(D/'worker-B/manifest.json',Q/'worker-manifest.json')
lines=before.splitlines(keepends=True);reads=[];offset=0
for first,last in [(1,95),(96,335),(336,583)]:
 raw=b''.join(lines[first-1:last]);assert len(raw)<=v.LIMIT
 reads.append(dict(lines=[first,last],bytes=[offset,offset+len(raw)],sha256=hashlib.sha256(raw).hexdigest()));offset+=len(raw)
assert offset==len(before)
dump(Q/'controller-read-ranges.json',dict(baseline_reads=reads,current_added_lines=[96,499],current_full_file_sha256=hashlib.sha256(current).hexdigest(),original_helpers_and_tests_preserved=True))
sizes=v.file_hashes(bp,R,Path(bp.header['source_repo']),target=False)
archive=Q/'authority-before';archive.mkdir()
for rel in [v.BLUEPRINT,v.SELECTOR,v.EVIDENCE+'/claims.json',*v.scaffold(bp,sizes,v.EVIDENCE)]:
 p=archive/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
(R/file.artifact).parent.mkdir(parents=True,exist_ok=True)
atomic(R/file.artifact,worker_report.read_text()+append)
refs=[record(R/file.artifact),record(Q/'review.md'),record(Q/'worker-report.md'),record(Q/'worker-manifest.json'),record(Q/'controller-read-ranges.json'),record(P/'review.md'),record(P/'master-verification.json'),record(P/'integration-evidence.tar.gz')]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='file133-current-integration-3.1.21',integrated_revision=v.repository_head(R),validators=bp.items[KEY].validators,source_path=file.path,source_hash=file.sha256,read_ranges=[r['bytes'] for r in reads],artifacts=refs)
for role in ['worker','master']:
 receipt={**base,'role':role,'reviewer':'controller' if role=='master' else 'worker B with controller current qualification'}
 if role=='master':receipt['manual_review']=dict(decision='accepted',reviewer='controller',evidence=record(Q/'review.md'),findings='Complete583-line baseline plus404-line increment and full report independently read; all helpers/12 original tests/5 new functions mapped, actual original12+newfailure and current13/43 comparison verified with fresh separate targets. Only133 file understanding; limitations and all product/directory obligations retained.')
 atomic(R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json',json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
 text,count=re.subn(r'^- \[[ _x]\]( \*\*ZS1-133\*\*)','- '+mark+r'\1',(R/v.BLUEPRINT).read_text(),flags=re.M);assert count==1
 updated=v.parse(text);assert updated.requirement==bp.requirement
 atomic(R/v.BLUEPRINT,text);active=read(R/v.SELECTOR);active['snapshot_sha256']=updated.snapshot;atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
 for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():atomic(R/rel,v.tsv(fields,rows).decode())
 claims=read(R/v.EVIDENCE/'claims.json')['claims'];front=v.frontiers(updated,claims,sel['run_id'],3)
 paths=set((R/v.EVIDENCE).glob('todos_*.md'));paths.add(R/v.EVIDENCE/f'todos_{datetime.date.today():%Y%m%d}.md')
 for path in paths:atomic(path,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))
 return updated
state('[_]');pre=v.validate(R,item=KEY);dump(Q/'before-promotion.json',pre);assert pre['ok'],pre
state('[x]');post=v.validate(R,item=KEY);dump(Q/'after-promotion.json',post);assert post['ok'],post
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY];start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(p.stdout);(Q/'gstage.stderr.log').write_bytes(p.stderr)
dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')))
assert p.returncode==0,p.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,scope='complete file133 understanding only',artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
print(json.dumps(dict(accepted=KEY,counts=post['counts'],product_public=str(P),file_public=str(Q))))

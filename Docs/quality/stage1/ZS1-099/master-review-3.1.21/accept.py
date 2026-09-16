from pathlib import Path
import datetime,hashlib,json,re,shutil,subprocess,sys,tarfile
R=Path(__file__).resolve().parents[2];sys.path.insert(0,str(R/'tools'));import validate_stage1_blueprint as v;import generate_stage1_gantt as g
D=R/'.ops/stage1_execution/file099-escape-3.1.21';Q=R/'Docs/quality/stage1/ZS1-099/master-review-3.1.21';assert not D.exists() and not Q.exists();D.mkdir()
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def read(p):return json.loads(p.read_text())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def rec(p):return dict(path=str(p.relative_to(R)),**meta(p))
W=Path('/Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target099-escape-understand-3.1.21-ready');assert meta(W/'manifest.json')['sha256']=='6821362d624841bf22b4670072cb7218ca3e6059df64d829516f10345248bb24'
shutil.copytree(W,D/'worker');W=D/'worker';argv=['python3','-B',str(W/'verify.py')];start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(D/'offline.stdout.log').write_bytes(p.stdout);(D/'offline.stderr.log').write_bytes(p.stderr);dump(D/'offline.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,stdout=meta(D/'offline.stdout.log'),stderr=meta(D/'offline.stderr.log')));assert p.returncode==0,p.stderr
KEY='ZS1-099';bp=v.parse((R/v.BLUEPRINT).read_text());sel=v.selector(R,bp,v.BLUEPRINT);f=bp.files[KEY]
assert bp.items[KEY].state=='[ ]' and all(bp.items[k].state=='[x]' for k in bp.items[KEY].depends)
assert not (R/f.artifact).exists()
for role in ['worker','master']:assert not (R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json').exists()
assert meta(W/f.artifact)['sha256']=='568e8cf7f69e103a1d5b889db9d990752ebb542a30529b82dbf15532a898ef5f'
assert meta(R/f.path)['sha256']=='41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f'
assert meta(W/'prior099/sources/before-mio.rs')['sha256']==f.sha256
append='''

## Controller independent complete file acceptance — current41321e, 3.1.21

The controller previously read the complete28350-byte original099 report and complete477-line1afa source, and now independently read the entire new appendix, complete178-line Esc patch and worker product review. In this acceptance turn the original9026-byte229-line frozen source was read again in full, along with current production polling/parser context and the entire unchanged readiness-test module. This covers original fields, construction/drop, all poll/read/error paths, parser state, original and added tests; unchanged blocks reuse their exact source identities rather than claiming a new unrecorded whole-file reread. The current655-line identity is reconstructed by the six contiguous ranges supplied with the worker chain; the manifest/ranges are integrity evidence, not a substitute for the semantic reading above.

The original source lost unconsumed edge-triggered readiness across early event returns and did not reliably handle EOF/other read errors. The first1afa change retained a pending token batch, avoided speculative blocking reads after progress with FIONREAD, kept parser events first, retired drained tokens, and returned EOF/read errors. The subsequent41321e change completes only a single pending Esc when that TTY token is actually drained; it does not consume incomplete UTF8/CSI/paste, discard already queued Alt suffixes or change fd flags, terminal mode, capacities, signal or wake ownership. The helper consumes only a successfully parsed sequence. Existing allocation capacities do not enforce bounds; repeated EINTR, endlessly incomplete paste, competitive consumers, initialization-error masking in the caller, size fallback and untested error injections retain the documented limits.

The worker report preserves the original static Esc suspicion and separately records the later reliable real-socket negative. Its public PTY before and after both pass; no per-read trace proves the cause of that difference, and no production-PTY failure is invented. All old source/read/test chains remain immutable. The current report-only verifier was fully read and executed once in a new copied package:262 payloads, three source versions, six contiguous current ranges,178 new lines, original report prefix and11 historical run-log bindings pass. No historical product program was rerun by this verifier.

After the worker's final identity snapshot, controller integration independently completed with main mio41321e, headless944049 and project-test8375:144 top-level Rust tests, vendor default8 and libc/event-stream9, Clippy and fmt pass. These overlapping vendor configurations are not17 independent tests. Root source and vendor lockfile checks have separate scope. Product evidence is published in ZS1-129/master-escape-boundary-3.1.21. No production release or formal budget has yet been executed for this combination; prior93f348 evidence remains historical. The earlier appendix statement that combined checks were still running is preserved as its captured-time statement, qualified here by the actual completed controller result.

Accept only099 complete file understanding of frozen57f4 baseline and current41321e source. The blueprint source hash remains57f4; it is not replaced with an implementation hash. The source has no project/session/persistence or model-transport ownership. TUI integration, full129/131,400–405 directories and whole Stage1 remain separately unfinished. Platform coverage is the actual macOS arm64 configurations described above; it does not establish Linux, all Unix features, use-dev-tty or Windows behavior. Those limitations belong to the file's understood contract, not hidden claims of whole-product completeness.
'''
Q.mkdir(parents=True);(Q/'review.md').write_text(append.lstrip());shutil.copy2(W/f.artifact,Q/'worker-report.md');shutil.copy2(W/'manifest.json',Q/'worker-manifest.json');shutil.copy2(W/'current-read-chain.json',Q/'current-read-chain.json');shutil.copy2(Path(__file__),Q/'accept.py')
for n in ['offline.stdout.log','offline.stderr.log','offline.run.json']:shutil.copy2(D/n,Q/n)
with tarfile.open(Q/'worker-ready.tar.gz','x:gz') as t:
 for p in sorted(W.rglob('*')):
  if p.is_file():t.add(p,arcname=str(p.relative_to(W)),recursive=False)
sizes=v.file_hashes(bp,R,Path(bp.header['source_repo']),target=False)
for rel in [v.BLUEPRINT,v.SELECTOR,v.EVIDENCE+'/claims.json',*v.scaffold(bp,sizes,v.EVIDENCE)]:
 p=Q/'authority-before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
# The completed099 task has been refilled to400; keep its new running claim separate.
ledger=read(R/v.EVIDENCE/'claims.json')
for c in ledger['claims']:
 if c['session']=='01a08be3-4678-75a0-9369-fbf8dbddb1ed':c.update(item_id='ZS1-400',owned_paths=list(bp.items['ZS1-400'].owned_paths),runtime_status='live',runtime_note='099逐文件理解已交主控独立验收；当前逐目录复核Unix source400，物理兄弟文件仅context。',runtime_checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
g.atomic(R/v.EVIDENCE/'claims.json',json.dumps(ledger,ensure_ascii=False,indent=2)+'\n')
(R/f.artifact).parent.mkdir(parents=True,exist_ok=True);g.atomic(R/f.artifact,(W/f.artifact).read_text()+append)
refs=[rec(R/f.artifact)]+[rec(Q/n) for n in ['review.md','worker-report.md','worker-manifest.json','current-read-chain.json','worker-ready.tar.gz','offline.stdout.log','offline.run.json','accept.py']]+[rec(R/'Docs/quality/stage1/ZS1-129/master-escape-boundary-3.1.21'/n) for n in ['review.md','master-verification.json','integration-evidence.tar.gz']]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='file099-three-source-versions-3.1.21',integrated_revision=v.repository_head(R),validators=bp.items[KEY].validators,source_path=f.path,source_hash=f.sha256,read_ranges=[[0,9026]],artifacts=refs)
for role in ['worker','master']:
 receipt={**base,'role':role,'reviewer':'controller' if role=='master' else 'worker A with controller current qualification'}
 if role=='master':receipt['manual_review']=dict(decision='accepted',reviewer='controller',evidence=rec(Q/'review.md'),findings='Complete frozen source and prior477-line understanding plus all178 new lines and current report reviewed; six exact-hash current ranges, real socket negative/PTY counterevidence and actual controller144 Rust checks preserved. Only099 file understanding accepted; no product/directory acceptance.')
 g.atomic(R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json',json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
 text,n=re.subn(r'^- \[[ _x]\]( \*\*ZS1-099\*\*)','- '+mark+r'\1',(R/v.BLUEPRINT).read_text(),flags=re.M);assert n==1
 updated=v.parse(text);assert updated.requirement==bp.requirement;g.atomic(R/v.BLUEPRINT,text);active=read(R/v.SELECTOR);active['snapshot_sha256']=updated.snapshot;g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
 for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():g.atomic(R/rel,v.tsv(fields,rows).decode())
 front=v.frontiers(updated,read(R/v.EVIDENCE/'claims.json')['claims'],sel['run_id'],3)
 for t in (R/v.EVIDENCE).glob('todos_*.md'):g.atomic(t,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))
state('[_]');pre=v.validate(R,item=KEY);dump(Q/'before-promotion.json',pre);assert pre['ok'],pre
state('[x]');post=v.validate(R,item=KEY);dump(Q/'after-promotion.json',post);assert post['ok'],post
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY];start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(p.stdout);(Q/'gstage.stderr.log').write_bytes(p.stderr);dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')));assert p.returncode==0,p.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
print(json.dumps(dict(accepted=KEY,counts=post['counts'],snapshot=post['snapshot_sha256'])))

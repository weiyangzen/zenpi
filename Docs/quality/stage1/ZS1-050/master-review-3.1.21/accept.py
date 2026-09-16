from pathlib import Path
import datetime,hashlib,json,re,shutil,subprocess,sys,tarfile
R=Path(__file__).resolve().parents[2];sys.path.insert(0,str(R/'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
D=R/'.ops/stage1_execution/escape-current-3.1.21';F=R/'.ops/stage1_execution/dir050-current-3.1.21'
P=R/'Docs/quality/stage1/ZS1-129/master-escape-boundary-3.1.21';Q=R/'Docs/quality/stage1/ZS1-050/master-review-3.1.21'
assert not P.exists() and not Q.exists()
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def read(p):return json.loads(p.read_text())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
def record(p):return dict(path=str(p.relative_to(R)),**meta(p))
after=read(D/'inputs-after.json');assert all(meta(R/p)==m for p,m in after.items())
for name in ['worker-offline','apply-check','apply','vendor-default','vendor-libc-stream','root-tests','root-clippy','root-fmt']:
 assert read(D/(name+'.run.json'))['exit_code']==0,name
counts=[int(n) for n in re.findall(r'test result: ok\. (\d+) passed; 0 failed', (D/'root-tests.log').read_text())]
assert counts==[13,43,7,19,16,46] and sum(counts)==144
for name,count in [('vendor-default',8),('vendor-libc-stream',9)]:assert f'{count} passed; 0 failed' in (D/(name+'.log')).read_text()
verification=read(D/'master-verification.json');verification.update(top_level_rust_tests=144,vendor_default=8,vendor_libc_event_stream=9,vendor_configurations_overlap=True)
dump(D/'master-verification.json',verification)
review='''# Controller: drained input Esc boundary integration — 3.1.21

Main now contains the exact mio1afaaa→41321e candidate, with headless944049 and project regression8375 retained. Only mio changes among224 captured build inputs. The controller read the entire worker review and all178 added patch lines, plus current production polling/parser context; original477-line understanding is reused through the previously reviewed099 exact-hash chain. The portable verifier itself was fully read and executed once in a new copied package. It checks all60 payloads,224 frozen inputs, actual negative/positive run-log bindings, previous read chain and a temporary-Git byte-exact roundtrip; it does not execute archived tests.

The15 production lines complete only a parser buffer equal to one Esc after the TTY readiness is drained by FIONREAD zero or WouldBlock. They invoke the existing parser with input_available=false and consume only the completed sequence. Previously decoded events retain priority, pending signal/wake tokens survive, and already queued Alt suffixes are read first. Incomplete UTF8/CSI/paste buffers are untouched. A suffix arriving later may form a separate event, matching the existing short-read ambiguity policy. No timeout, buffer, fd flags, terminal mode or reset API changed. Existing errors, continuous EINTR, arbitrary concurrent consumers and endless partial paste retain their earlier limits.

The preserved worker socket negative is concrete: one write1023x+Esc, all x delivered, then None instead of Esc; 2 other tests passed and this test failed. The same negative test stops at its nonblocking branch, so its blocking negative is not claimed. The worker public event::poll/read PTY before and after both passed with one1024-byte write and restored termios; no per-read trace exists to establish why this PTY did not reproduce. This counterevidence remains intact, and old binaries/old matrices are not rerun here.

Controller actual main-combination checks pass: default vendor8 and libc/event-stream9 tests, including original readiness/reset coverage and new bounded socket/partial-sequence/wake checks. The configurations overlap and are not17 independent scenarios. Wake token ordering is explicitly arranged after genuine kernel events, not claimed naturally observed in all permutations. Both vendor builds used a new isolated target and vendor lockfile; root tests separately used the root lockfile and the target owned by this main checkout. Six root integration suites pass13+43+7+19+16+46=144 top-level tests, covering project/headless protocol/layout/slash/BentoBox/composer. The self-spawn project child is included in13. All-target Clippy with warnings denied and cargo fmt pass. Input hashes are unchanged at completion.

This is a local source integration, not whole129/131/099 acceptance. No new production release or budget has run for41321e+944049. Prior93f348 budget/PTY results remain prior-revision evidence, with old failures retained. The Esc source-understanding report099 and every enclosing directory still require their own acceptance. Main rollback would remove only this exact15-production/163-test increment, preserving older readiness and all project UX work.
'''
(D/'review.md').write_text(review);P.mkdir(parents=True)
for n in ['review.md','master-verification.json','inputs-before.json','inputs-after.json','root-tests.log','root-tests.run.json','root-clippy.run.json','root-fmt.run.json','vendor-default.log','vendor-default.run.json','vendor-libc-stream.log','vendor-libc-stream.run.json']:shutil.copy2(D/n,P/n)
shutil.copy2(R/'.ops/stage1_execution/integrate_escape_321_current.py',P/'integrate.py')
with tarfile.open(P/'integration-evidence.tar.gz','x:gz') as t:
 for p in sorted(D.rglob('*')):
  rel=p.relative_to(D)
  if rel.parts[0]!='vendor-target' and p.is_file():t.add(p,arcname=str(rel),recursive=False)
dump(P/'manifest.json',dict(complete=False,artifacts={str(p.relative_to(P)):meta(p) for p in P.rglob('*') if p.is_file()}))

KEY='ZS1-050';bp=v.parse((R/v.BLUEPRINT).read_text());sel=v.selector(R,bp,v.BLUEPRINT)
scope,folder,report=bp.folders[KEY]
assert bp.items[KEY].state=='[ ]' and bp.items[KEY].depends==('ZS1-012',)
assert bp.items['ZS1-012'].state=='[x]' and not (R/report).exists()
for role in ['worker','master']:assert not (R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json').exists()
worker=F/'worker';candidate=worker/'files'/report
assert meta(candidate)['sha256']=='6308e6745d722b3eabfa9b7426bb9591de9d95e93532d0a12da9a00446796733'
assert read(F/'offline.run.json')['exit_code']==0 and read(F/'offline.stdout.json')['passed']
for row in read(worker/'capture.json')['direct_entries']:
 assert meta(Path(bp.header['source_repo'])/folder/row['name'])=={k:row[k] for k in ['bytes','sha256']}
assert meta(R/'Docs/learn/stage1_pi_mono/receipts/ZS1-012.master.json')['sha256']=='0f8fc23e6c09a9db20c67e4d0a819a8df33d4d18dcc9c98d53c1075194d6f782'
append='''

## Controller independent directory acceptance — 3.1.21

The controller independently read this entire22285-byte candidate, all300 lines of branch-summarization.ts and132 lines of utils.ts, the complete historical directory.test.ts and all five observations, original successful stdout and the initial two-failure stack. The original865-line compaction.ts full reading and37 actual master tests are reused via accepted012, whose exact27410-byte source and current receipt/artifact chain were independently checked. That prior master review was read again; no new complete compaction-source or37-test replay is claimed. The complete portable verifier was read before executing it once in a new copied package:25 offline checks passed, with no historical runtime, npm, provider, product or TUI execution.

The physical directory has exactly three files and no subdirectory. Frozen child closure is only012. The two siblings supply context, not additional accepted files. Branch preparation collects call intent even for excluded/error tool results; its tags reach prompt text without becoming compaction typed details. Previous compaction details have a separate inheritance path. Shared nested references, lossy serialization, heuristic cuts/budgets, cooperative cancellation, ordinary promise rejection and missing directory-owned persistence are explicitly preserved as limits. D050-01 calls prepareBranchEntries, not collectEntriesForBranchSummary, despite its old name. D050-02 JSON reconstruction is not a journal restart. The recorded fixture-only AbortSignal/context argument correction retains the original3-pass/2-fail result before5-pass; no product defect or fresh replay is inferred.

For the cross-directory boundary the controller read actual upstream lane admission675–830/825–935, structural request800–895, publication210–390 and recovery/threshold1065–1195 excerpts, plus current Zenpi session1237–1385 and core1880–2035. These establish caller-selected ancestry, durable preparation, request/gate/usage ownership, entry-and-tip publication through lane continuation, orphaned-attempt policy, and Zenpi operation/provenance/cancel checks before checkpoint append/restore. Other unchanged context is reused from accepted012 and the worker's bounded context review, with all ten current context identities independently checked. This is not complete acceptance of those caller/target files or proof of real transactional recovery/model semantic fidelity.

Accept only050 directory understanding, dependent on already accepted012.054, other ancestors, source siblings, target files and product103/104 remain independently open. The current main Esc integration changes mio only and does not alter any of these captured directory/context bytes; no Esc evidence is borrowed to satisfy050. Worker historical authority and provisional text above remain unchanged, qualified by this current master decision. This report and its separately archived evidence are the acceptance scope; offline counts alone do not establish semantic truth.
'''
Q.mkdir(parents=True);(Q/'review.md').write_text(append.lstrip());shutil.copy2(candidate,Q/'worker-report.md');shutil.copy2(worker/'manifest.json',Q/'worker-manifest.json')
for n in ['offline.stdout.json','offline.stderr.log','offline.run.json']:shutil.copy2(F/n,Q/n)
shutil.copy2(Path(__file__),Q/'accept.py');shutil.copy2(R/'.ops/stage1_execution/prepare_directory050_321.py',Q/'prepare.py')
with tarfile.open(Q/'worker-ready.tar.gz','x:gz') as t:
 for p in sorted(worker.rglob('*')):
  if p.is_file():t.add(p,arcname=str(p.relative_to(worker)),recursive=False)
sizes=v.file_hashes(bp,R,Path(bp.header['source_repo']),target=False)
for rel in [v.BLUEPRINT,v.SELECTOR,v.EVIDENCE+'/claims.json',*v.scaffold(bp,sizes,v.EVIDENCE)]:
 p=Q/'authority-before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
(R/report).parent.mkdir(parents=True,exist_ok=True);g.atomic(R/report,candidate.read_text()+append)
refs=[record(R/report)]+[record(Q/n) for n in ['review.md','worker-report.md','worker-manifest.json','worker-ready.tar.gz','offline.stdout.json','offline.run.json','accept.py','prepare.py']]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='directory050-current-3.1.21',integrated_revision=v.repository_head(R),validators=bp.items[KEY].validators,scope=scope,folder_path=folder,children=list(bp.items[KEY].depends),artifacts=refs)
for role in ['worker','master']:
 receipt={**base,'role':role,'reviewer':'controller' if role=='master' else 'worker B with historical A evidence and current controller qualification'}
 if role=='master':receipt['manual_review']=dict(decision='accepted',reviewer='controller',evidence=record(Q/'review.md'),findings='Independent full current directory report, both context siblings, five historical tests/observations and bounded current caller mapping reviewed; exact accepted012 chain reused. Three physical files/no subdirectories, only012 in scope. Offline25/25 not runtime; only050 accepted.')
 g.atomic(R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json',json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
 text,count=re.subn(r'^- \[[ _x]\]( \*\*ZS1-050\*\*)','- '+mark+r'\1',(R/v.BLUEPRINT).read_text(),flags=re.M);assert count==1
 updated=v.parse(text);assert updated.requirement==bp.requirement
 g.atomic(R/v.BLUEPRINT,text);active=read(R/v.SELECTOR);active['snapshot_sha256']=updated.snapshot;g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
 for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():g.atomic(R/rel,v.tsv(fields,rows).decode())
 front=v.frontiers(updated,read(R/v.EVIDENCE/'claims.json')['claims'],sel['run_id'],3)
 for t in (R/v.EVIDENCE).glob('todos_*.md'):g.atomic(t,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))
state('[_]');pre=v.validate(R,item=KEY);dump(Q/'before-promotion.json',pre);assert pre['ok'],pre
state('[x]');post=v.validate(R,item=KEY);dump(Q/'after-promotion.json',post);assert post['ok'],post
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY];start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(p.stdout);(Q/'gstage.stderr.log').write_bytes(p.stderr)
dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')))
assert p.returncode==0,p.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
print(json.dumps(dict(accepted=KEY,counts=post['counts'],snapshot=post['snapshot_sha256'],product_public=str(P),directory_public=str(Q))))

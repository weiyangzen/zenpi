from pathlib import Path
import datetime, hashlib, json, re, shutil, subprocess, sys, tarfile
R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
KEY = 'ZS1-402'
D = R / '.ops/stage1_execution/directory402-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-402/master-review-3.1.21'
assert not Q.exists()
def read(p): return json.loads(p.read_text())
def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
def dump(p, data): p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
def record(p): return dict(path=p.relative_to(R).as_posix(), **meta(p))
assert meta(W / 'manifest.json')['sha256'] == 'c401bb83e60305fb3439b109d8cf2a12200791bc8305365295c196f094b93fe2'
assert read(D / 'offline.run.json')['exit_code'] == 0 and read(D / 'offline.stdout.json')['ok']
for row in read(W / 'manifest.json')['files']: assert meta(W / row['path']) == {k: row[k] for k in ['bytes','sha256']}
bp = v.parse((R / v.BLUEPRINT).read_text()); sel = v.selector(R, bp, v.BLUEPRINT)
scope, folder, report = bp.folders[KEY]
assert scope == 'target' and bp.items[KEY].state == '[ ]'
assert bp.items[KEY].depends == ('ZS1-401',) and bp.items['ZS1-401'].state == '[x]'
assert not (R / report).exists()
for role in ['worker','master']: assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
candidate = W / report
assert meta(candidate)['sha256'] == 'ea27c7edfca7ed007bddd25a9712d1315f5f898f6433b9dbd55ee80ea0d5045e'
inventory = read(W / 'physical-inventory.json')
entries = sorted((R / folder).iterdir())
assert {p.name for p in entries} == {x['name'] for x in inventory}
assert all(not p.is_symlink() for p in entries)
assert sum(p.is_file() for p in entries) == 6 and sum(p.is_dir() for p in entries) == 2
for row in inventory:
    p = R / row['path']
    if row['kind'] == 'file': assert meta(p) == {k: row[k] for k in ['bytes','sha256']}
    else:
        assert {x.name for x in p.iterdir()} == {x['name'] for x in row['direct_children']}
        assert all(not x.is_symlink() for x in p.iterdir())
current = read(W / 'related-source-identities.json')
for rel, m in current.items(): assert meta(R / rel) == {k: m[k] for k in ['bytes','sha256']}
for key in ['400','401']:
    rel = v.EVIDENCE + f'/receipts/ZS1-{key}.master.json'
    assert (R / rel).read_bytes() == (W / f'accepted{key}' / rel).read_bytes()
    for artifact in read(R / rel)['artifacts']: v.artifact(R, artifact)
append = '''

## Controller independent directory acceptance — 3.1.21

The controller read this complete25390-byte candidate and its entire portable verifier. It newly read every line of event.rs1777, Unix parse.rs1506, lib.rs263, cursor/sys/unix.rs56 and terminal/sys/unix.rs326:3928 lines of actual module roots, parser functions, query callers and all included tests. These five context files are not new formal leaf acceptances. The complete400 and401 master reviews were read again. Their previously independently read20 complete context files and accepted099 baseline/current655-line chain are reused by exact current source and receipt-artifact identity, without executing archived programs or claiming fresh rereads of those20 files.

An independently enumerated physical event/ has six regular files and two direct directories, no symlink or additional hidden entry. event.rs is its parent's file. There are zero frozen direct files and only401/source as formal child; sys/ and all six files remain context. Both400 and401 are individually accepted, their exact9/10 artifact chains checked. Their acceptance is a prerequisite, not automatic402 acceptance. Current source matches all25 fresh/reused context identities and41321e mio. The root ran the new copied package's read-only verifier once with external logs:438 payloads verified, including original failures, old state transitions,14 continuous new read blocks and20 reused full contexts. Hash coverage is supporting evidence; the separate semantic reading is the basis of this decision.

The actual module root gates event on events and EventStream on event-stream; source/sys and waker selectors must agree on target/use-dev-tty. The public reader is one process-global Mutex<Option<InternalEventReader>>. Lazy initialization occurs while acquiring the mapped guard. poll subtracts lock-acquisition time, but source construction, size lookup and arbitrary blocking operations are not made preemptible by that duration. read can wait indefinitely, and poll followed by read is not an atomic multi-reader transaction. Public documentation excludes cross-thread synchronous readers or mixing EventStream. Source parser/readiness, reader events/skipped state, async executor waker and OS waker are distinct owners. The previously reviewed constructor .ok error erasure, filtered queue priority, error-time skipped-event loss and no automatic alternate backend fallback remain explicit.

The full parser read confirms incremental byte parsing and input_available's special standalone-Esc role.099's already accepted normal-drain fix targets only a single Esc; it does not flush arbitrary partial UTF8, CSI or paste. CR and raw-mode LF differ, Unicode uppercase detection affects parsed SHIFT, whereas KeyEvent Eq/Hash use ASCII case normalization and do not mutate the received event. Repeat and Release remain separate kinds. Native paste accumulates until its end marker with lossy UTF8 conversion and no explicit total cap in this parser. CSI-u handles codepoint, modifiers/kind, shifted alternative, keypad/media/modifier and lock state, but does not emit the associated-text component. The actual tests verify specific examples and equality can conceal literal field differences; their presence is not a new execution result.

Two parser limitations are confirmed statically and stay open for product follow-up: cursor/RXVT/SGR coordinates subtract1 from parsed u16 without rejecting zero; X10 saturates the byte offset then subtracts1. This can panic with overflow checks or wrap in release. Enhancement flag parsing inspects the first ASCII digit's bits, so multi-digit flags are not parsed as the intended whole decimal value. No malformed-input experiment was run for402 and neither defect is declared repaired. No exhaustive platform, parser robustness or TUI acceptance follows from understanding this directory.

Cursor position query writes/flushed stdout, can temporarily enable raw mode and returns restoration failure ahead of a stored query result. Each poll has2000ms but error branches retry; there is no single function-wide deadline. Keyboard enhancement query tries /dev/tty then stdout, waits for flags or DA, and after flags uses unbounded read_internal to consume DA. Repeated poll errors and the second DA wait therefore are not covered by a blanket two-second guarantee. Raw mode has a separate saved-Termios mutex and is not reference counted or reconciled with external termios changes. Resize may reach tput, whose process wait has no local timeout and whose numeric fold does not validate arbitrary output/overflow or child exit success. These are actual query/size ownership limits, not new tested failures.

reset_event_reader uses try_lock and drops the reader under that guard. It clears in-process completed/partial/readiness state but does not flush OS input, alter raw/mouse/focus/paste modes, join other readers, persist a draft or coordinate queries. Both cursor and keyboard enhancement queries must be stopped externally. The reset API is absent with event-stream. Its existing100ms lock-contention test does not prove every destructor or terminal handoff is bounded. EventStream Drop signals/wakes without join and ignores some failures; public synchronous/async mixing restrictions remain. Windows command stubs/Unsupported paths and actual ANSI enable/disable commands do not imply constructing/destroying a source changes terminal modes.

This directory has no session/project correlation, WAL, provider lifecycle, persistent input queue or draft owner. Historical144 Rust and overlapping8/9 vendor tests,6891 projectPTY12/kill-yank29 and later92f95 product results remain bound to their own revisions. No runtime, build, provider, PTY, budget or old probe was executed by this402 acceptance. Headless pending-input repair remains a separate active product candidate, not integrated by this review. Original worker packing failures when401 became accepted and a verifier was not yet generated remain archived; the controller's initial read used an inapplicable files/ prefix and failed before correctly reading this package's direct Docs/ report layout.

Accept402 only after independently accepted401.403/404/405/root, six context-only files, sys directory,129/131 and the full goal remain separately open. This records complete understanding of the frozen directory and its real limits, not complete product parity. Rollback withdraws402 report/receipts/status only, preserving all child acceptance, product source, original failures and user state.
'''
Q.mkdir(parents=True); (Q/'review.md').write_text(append.lstrip())
shutil.copy2(candidate,Q/'worker-report.md'); shutil.copy2(W/'manifest.json',Q/'worker-manifest.json'); shutil.copy2(Path(__file__),Q/'accept.py')
for name in ['offline.stdout.json','offline.stderr.log','offline.run.json']: shutil.copy2(D/name,Q/name)
dump(Q/'current-context.json',current)
with tarfile.open(Q/'worker-ready.tar.gz','x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file(): archive.add(p,arcname=p.relative_to(W).as_posix(),recursive=False)
sizes=v.file_hashes(bp,R,Path(bp.header['source_repo']),target=False)
for rel in [v.BLUEPRINT,v.SELECTOR,v.EVIDENCE+'/claims.json',*v.scaffold(bp,sizes,v.EVIDENCE)]:
    p=Q/'authority-before'/rel;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(R/rel,p)
(R/report).parent.mkdir(parents=True,exist_ok=True);g.atomic(R/report,candidate.read_text()+append)
refs=[record(R/report)]+[record(Q/name) for name in ['review.md','worker-report.md','worker-manifest.json','worker-ready.tar.gz','offline.stdout.json','offline.run.json','current-context.json','accept.py']]
base=dict(schema_version='stage1-receipt/v1',item_id=KEY,run_id=sel['run_id'],requirement_digest=bp.requirement,baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],complete=True,attempt_id='directory402-current-3.1.21',integrated_revision=v.repository_head(R),validators=bp.items[KEY].validators,scope=scope,folder_path=folder,children=list(bp.items[KEY].depends),artifacts=refs)
for role in ['worker','master']:
    receipt=dict(base,role=role,reviewer='controller' if role=='master' else 'worker A with independent controller qualification')
    if role=='master': receipt['manual_review']=dict(decision='accepted',reviewer='controller',evidence=record(Q/'review.md'),findings='Full402 report/verifier plus3928 new context lines read.20 exact previously read contexts and099 chain reused.401/400 actual receipts verified.438 offline payloads pass.6files/2dirs, only401 formal child.Only402 understood; parser/query limitations explicitly open.')
    g.atomic(R/v.EVIDENCE/'receipts'/f'{KEY}.{role}.json',json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')
def state(mark):
    text,count=re.subn(r'^- \[[ _x]\]( \*\*ZS1-402\*\*)','- '+mark+r'\1',(R/v.BLUEPRINT).read_text(),flags=re.M);assert count==1
    updated=v.parse(text);assert updated.requirement==bp.requirement;g.atomic(R/v.BLUEPRINT,text)
    active=read(R/v.SELECTOR);active['snapshot_sha256']=updated.snapshot;g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():g.atomic(R/rel,v.tsv(fields,rows).decode())
    claims=read(R/v.EVIDENCE/'claims.json');front=v.frontiers(updated,claims['claims'],sel['run_id'],3)
    for todo in (R/v.EVIDENCE).glob('todos_*.md'):g.atomic(todo,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))
state('[_]');pre=v.validate(R,item=KEY);dump(Q/'before-promotion.json',pre);assert pre['ok'],pre
state('[x]');post=v.validate(R,item=KEY);dump(Q/'after-promotion.json',post);assert post['ok'],post
argv=['python3','tools/validate_stage1_blueprint.py','--item',KEY];start=datetime.datetime.now(datetime.timezone.utc).isoformat();p=subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(p.stdout);(Q/'gstage.stderr.log').write_bytes(p.stderr)
dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')));assert p.returncode==0,p.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,artifacts={p.relative_to(Q).as_posix():meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main();print(json.dumps(dict(accepted=KEY,counts=post['counts'],snapshot=post['snapshot_sha256'])))

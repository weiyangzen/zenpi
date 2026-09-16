from pathlib import Path
import datetime, difflib, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-096'
D = R / '.ops/stage1_execution/target096-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-096/master-review-3.1.21'
assert not Q.exists()

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def read(p):
    return json.loads(p.read_text())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

def rec(p):
    return dict(path=str(p.relative_to(R)), **meta(p))

bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
f = bp.files[KEY]
assert bp.requirement == '3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d'
assert sum(i.state == '[x]' for i in bp.items.values()) == 53
assert bp.items[KEY].state == '[ ]'
assert all(bp.items[k].state == '[x]' for k in bp.items[KEY].depends)
assert not (R / f.artifact).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
assert meta(W / 'manifest.json')['sha256'] == '4b3fc05994c41164db23839fbf1a8fc7c802db26b801c034d890ec8d281e9d48'
assert meta(W / 'files' / f.artifact)['sha256'] == '248b6883f7e3910718c1be101151f4c660c738c8f51cf8b66ce4eed29df81d47'
source = R / f.path
baseline = W / read(W / 'baseline-comparison.json')['baseline_path']
captured = W / 'capture/zenpi/src/render.rs'
assert meta(captured) == dict(bytes=60098,sha256='b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d')
fix = R / '.ops/stage1_execution/preview-grapheme-integration-3.1.21'
delta = ''.join(difflib.unified_diff(captured.read_text().splitlines(True),source.read_text().splitlines(True),fromfile='a/src/render.rs',tofile='b/src/render.rs'))
assert delta == (fix/'product.patch').read_text()
assert meta(source) == read(fix/'inputs-after.json')['src/render.rs']
assert meta(source) == dict(bytes=59562, sha256='9e526c42e1394cc2aa888f9627e7e0dfaf5b5e3acd0db3187c65553884d61c0a')
assert meta(baseline) == dict(bytes=30217, sha256=f.sha256)
assert f.sha256 == '6eaaa712745659ae48f429217c5eb8961dd12e71789f8a2bc70867ee71ba25a7'
new = source.read_bytes().splitlines(keepends=True)
assert len(new) == 1688 and len(baseline.read_bytes().splitlines()) == 901
run = read(D / 'root-offline/run.json')
audit = [json.loads(line) for line in (D / 'root-offline/stdout.log').read_text().splitlines()]
assert run['exit_code'] == 0 and audit[-1] == dict(checks=233,failed=0,structural_only=True,runtime_execution=False)
assert all(row['passed'] for row in audit[:-1])
assert not (D / 'root-offline/stderr.log').read_bytes()
for name in ['render_markdown.rs','tui_markdown.rs']:
    assert (R/'tests'/name).read_bytes() == (W/'capture/zenpi/tests'/name).read_bytes()
review = (D / 'root-review.md').read_text()

Q.mkdir(parents=True)
(Q / 'review.md').write_text(review)
shutil.copyfile(W / 'files' / f.artifact, Q / 'worker-report.md')
shutil.copyfile(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copyfile(source, Q / 'current-render.rs')
shutil.copyfile(baseline, Q / 'baseline-render.rs')
shutil.copyfile(Path(__file__), Q / 'accept.py')
shutil.copyfile(W / 'baseline-to-current.diff', Q / 'baseline-to-current.diff')
for name in ['render_markdown.rs','tui_markdown.rs']:
    shutil.copyfile(R/'tests'/name,Q/name)
shutil.copyfile(captured,Q/'pre-fix-render.rs')
(Q/'preview-grapheme.patch').write_text(delta)
shutil.copyfile(D / 'root-offline/audit.py', Q / 'root-audit.py')
for n in ['stdout.log', 'stderr.log', 'run.json']:
    shutil.copyfile(D / 'root-offline' / n, Q / ('offline-' + n))
old_lines = captured.read_bytes().splitlines(keepends=True)
ops = difflib.SequenceMatcher(a=old_lines,b=new,autojunk=False).get_opcodes()
assert all(tag == 'equal' or (1108 <= a <= b <= 1147 and 1108 <= c <= d <= 1129) for tag,a,b,c,d in ops)
dump(Q/'read-binding.json',dict(
    formal_source=rec(Q/'current-render.rs'), pre_fix_source=rec(Q/'pre-fix-render.rs'),
    pre_fix_line_ranges=[[1,300],[301,600],[601,900],[901,1200],[1201,1450],[1451,1706]],
    pre_fix_tool_chunks=['bee85e','f71bb8','6503af','6e7a9c','7ec500','896348'],
    current_coverage=[dict(kind=tag,previous_lines=[a+1,b],current_lines=[c+1,d],
        current_byte_range=[sum(map(len,new[:c])),sum(map(len,new[:d]))],
        evidence='complete prior sequential read' if tag=='equal' else 'bccab5 and7009bd complete delta/function read') for tag,a,b,c,d in ops],
    baseline=rec(Q/'baseline-render.rs'),baseline_full_read_ranges=[[1,310],[311,620],[621,901]],
    baseline_read_tool_chunks=['ca1fd4','f7a156','32e84a'],
    tests_full_read_files=['tests/render_markdown.rs','tests/tui_markdown.rs'],tests_tool_chunk='d53ee6',
    report=rec(Q/'worker-report.md'),report_line_ranges=[[1,100],[101,200],[201,280]],
    report_read_tool_chunks=['857a68','61e217','85c552'],product_executions_this_turn=0))
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file():
            archive.add(p, arcname=str(p.relative_to(W)), recursive=False)
sizes = v.file_hashes(bp, R, Path(bp.header['source_repo']), target=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    dest = Q / 'authority-before' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(R / rel, dest)
(R / f.artifact).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / f.artifact, (W / 'files' / f.artifact).read_text() + '\n\n' + review)
refs = [rec(R / f.artifact)] + [rec(Q / n) for n in [
    'review.md','worker-report.md','worker-manifest.json','current-render.rs',
    'baseline-render.rs','read-binding.json','accept.py','worker-ready.tar.gz',
    'offline-stdout.log','offline-run.json','baseline-to-current.diff','render_markdown.rs','tui_markdown.rs','pre-fix-render.rs','preview-grapheme.patch','root-audit.py']]
refs.append(rec(R/'Docs/quality/stage1/ZS1-125/master-preview-grapheme-3.1.21/review.md'))
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'],
    requirement_digest=bp.requirement, baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],
    complete=True, attempt_id='target096-current-independent-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators,
    source_path=f.path, source_hash=f.sha256, read_ranges=[[0,30217]], artifacts=refs)
for role in ['worker','master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker C with controller current qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=rec(Q/'review.md'),
            findings='Complete1706-line pre-fix render and exact current1688-line derivation,901-line frozen baseline,280-line report and131-line external tests independently read; bounded actual TUI consumers read.233 fresh structural checks pass,0 new product runs; prior85Rust localfix separately bound; only096 file understanding accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-096\*\*)', '- '+mark+r'\1', (R/v.BLUEPRINT).read_text(), flags=re.M)
    assert count == 1
    updated = v.parse(text)
    assert updated.requirement == bp.requirement
    g.atomic(R/v.BLUEPRINT,text)
    active = read(R/v.SELECTOR)
    active['snapshot_sha256'] = updated.snapshot
    g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():
        g.atomic(R/rel,v.tsv(fields,rows).decode())
    front = v.frontiers(updated,read(R/v.EVIDENCE/'claims.json')['claims'],sel['run_id'],3)
    for p in (R/v.EVIDENCE).glob('todos_*.md'):
        g.atomic(p,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))

state('[_]')
pre = v.validate(R,item=KEY)
dump(Q/'before-promotion.json',pre)
assert pre['ok'],pre
state('[x]')
post = v.validate(R,item=KEY)
dump(Q/'after-promotion.json',post)
assert post['ok'],post
argv = ['python3','tools/validate_stage1_blueprint.py','--item',KEY]
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
result = subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(result.stdout)
(Q/'gstage.stderr.log').write_bytes(result.stderr)
dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=result.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')))
assert result.returncode == 0,result.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main()
print(json.dumps(dict(accepted=KEY,counts=post['counts'],snapshot=post['snapshot_sha256'])))

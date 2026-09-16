from pathlib import Path
import hashlib, json, re, shutil, sys, tarfile, importlib

R = Path(__file__).resolve().parents[3]
D = Path(__file__).resolve().parent
Q = R / 'Docs/quality/stage1/ZS1-125/master-preview-grapheme-3.1.21'
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def read(p):
    return json.loads(p.read_text())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

assert not Q.exists()
inputs = read(D / 'inputs-after.json')
assert {p: meta(R / p) for p in inputs} == inputs
before = read(D / 'before-run.json')
after = read(D / 'after-runs.json')
assert before['exit_code'] == 101
assert '0 passed; 3 failed;' in (D / 'before-stdout.log').read_text()
assert before['tests_sha256'] == meta(R / 'tests/tui_preview_graphemes.rs')['sha256']
assert [r['name'] for r in after] == ['render', 'tui', 'fmt', 'clippy', 'debug']
assert all(r['exit_code'] == 0 and r['inputs_unchanged'] for r in after)
counts = re.findall(r'test result: ok\. (\d+) passed; 0 failed; 0 ignored;',
                    (D / 'render.stdout.log').read_text() + (D / 'tui.stdout.log').read_text())
assert list(map(int, counts)) == [23, 12, 19, 3, 17, 11]
assert sum(map(int, counts)) == 85
assert meta(R / 'target/debug/zenpi') == read(D / 'binary.json')
assert (R / 'src/tui.rs').read_bytes() == (D / 'before/src/tui.rs').read_bytes()
assert meta(R / 'src/render.rs')['sha256'] == '9e526c42e1394cc2aa888f9627e7e0dfaf5b5e3acd0db3187c65553884d61c0a'
bp = v.parse((R / v.BLUEPRINT).read_text())
v.selector(R, bp, v.BLUEPRINT)
assert sum(i.state == '[x]' for i in bp.items.values()) == 53
assert bp.items['ZS1-125'].state == '[ ]' and bp.items['ZS1-096'].state == '[ ]'
Q.mkdir(parents=True)
for p in sorted(D.iterdir()):
    if p.is_file():
        shutil.copyfile(p, Q / p.name)
shutil.copytree(D / 'after', Q / 'current')
with tarfile.open(Q / 'root-package.tar.gz', 'x:gz') as archive:
    for p in sorted(D.rglob('*')):
        if p.is_file() and not any(part.startswith('fixture-') for part in p.relative_to(D).parts):
            archive.add(p, arcname=str(p.relative_to(D)), recursive=False)
summary = dict(item='ZS1-125', full_item_accepted=False, rust_passed=85,
               before_failed=3, actual_testbackend_frames=2, pty_executions=0,
               release_executions=0, binary=read(D / 'binary.json'), inputs_unchanged=True,
               review=str((Q / 'review.md').relative_to(R)))
dump(D / 'master-verification.json', summary)
dump(Q / 'master-verification.json', summary)
dump(Q / 'manifest.json', dict(item='ZS1-125', full_item_accepted=False,
     artifacts={str(p.relative_to(Q)): meta(p) for p in Q.rglob('*') if p.is_file()}))
generator = R / 'tools/generate_stage1_gantt.py'
lines = generator.read_text().splitlines(True)
matches = [i for i, line in enumerate(lines) if line.startswith("    'ZS1-125':")]
assert len(matches) == 1
lines[matches[0]] = "    'ZS1-125': ('正文查看器与审批预览窄屏emoji裁字已修复：原3失败→85项Rust通过，两个实际TestBackend画面保留末尾字符；原审批计时/提示修复保留，新PTY待验，整项待验', 'preview-grapheme-integration-3.1.21', 'Docs/quality/stage1/ZS1-125/master-preview-grapheme-3.1.21/review.md'),\n"
generator.write_text(''.join(lines))
importlib.reload(g).main()
print(json.dumps(dict(public=str(Q), manifest=meta(Q / 'manifest.json'), rust=85, full_item_accepted=False)))

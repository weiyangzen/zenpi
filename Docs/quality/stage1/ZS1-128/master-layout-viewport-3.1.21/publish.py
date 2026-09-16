from pathlib import Path
import hashlib,json,re,shutil,sys,tarfile
R=Path(__file__).resolve().parents[3];D=Path(__file__).resolve().parent
sys.path.insert(0,str(R/'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
Q=R/'Docs/quality/stage1/ZS1-128/master-layout-viewport-3.1.21'
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,d):p.write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def read(p):return json.loads(p.read_text())
assert not Q.exists()
inputs=read(D/'root-after-inputs.json');assert {p:meta(R/p) for p in inputs}==inputs
result=read(D/'root-after-result.json');pty=read(D/'root-pty-evidence/result.json')
assert result['passed'] and pty['error'] is None and len(pty['checks'])==21 and all(c['pass_'] for c in pty['checks'])
assert pty['http_requests']==[] and pty['child']['exit_code']==0 and pty['child']['reaped'] and pty['keeper']['exit_code']==0
assert meta(R/'target/debug/zenpi')==result['binary']==pty['binary']
counts=re.findall(r'test result: ok\. (\d+) passed; 0 failed;', (D/'root-after-regression.log').read_text())
assert len(counts)==10 and sum(map(int,counts))==171
for n in ['root-after-regression','root-after-format','root-after-clippy','root-after-build','root-pty']:
 assert read(D/(n+'.run.json'))['exit_code']==0
assert read(D/'root-before.run.json')['exit_code']==101
bp=v.parse((R/v.BLUEPRINT).read_text());assert sum(x.state=='[x]' for x in bp.items.values())==51
assert bp.items['ZS1-128'].state=='[ ]' and bp.items['ZS1-094'].state=='[x]'
Q.mkdir(parents=True)
for n in ['review.md','root.patch','rollback.patch','initial-inputs.json','root-before-inputs.json','root-after-inputs.json','root-before.log','root-before.run.json','root-before.py','root-after.py','root-after-regression.log','root-after-regression.run.json','root-after-format.log','root-after-format.run.json','root-after-clippy.log','root-after-clippy.run.json','root-after-build.log','root-after-build.run.json','root-after-result.json','root_pty_layout_viewport.py','pty_keeper.py','pty-static-review.json','root-pty.run.json','root-pty.log','publish.py']:
 shutil.copyfile(D/n,Q/n)
shutil.copytree(D/'root-pty-evidence',Q/'pty')
with tarfile.open(Q/'root-package.tar.gz','x:gz') as archive:
 for p in sorted(D.rglob('*')):
  if p.is_file() and 'root-bin' not in p.relative_to(D).parts:
   archive.add(p,arcname=str(p.relative_to(D)),recursive=False)
validation=v.validate(R,item='ZS1-094');assert validation['ok'],validation
dump(Q/'authority-validation.json',validation)
summary=dict(result,scope='128 local repair only; full128 pending',rust_passed=171,pty_passed=21,before_failures=2,before_control_pass=1,review=str((Q/'review.md').relative_to(R)))
dump(D/'master-verification.json',summary)
dump(Q/'manifest.json',dict(item='ZS1-128',full_item_accepted=False,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
p=R/'tools/generate_stage1_gantt.py';text=p.read_text()
lines=text.splitlines(True);matches=[i for i,s in enumerate(lines) if s.startswith("    'ZS1-128':")];assert len(matches)==1
lines[matches[0]]="    'ZS1-128': ('矮窗口键盘焦点修复已合入：两项原失败→171项Rust通过，新debug真实PTY21项通过；放大/缩小、窄屏循环与焦点持久化已验证，BentoBox保持，整项待验', 'layout-viewport-integration-3.1.21', 'Docs/quality/stage1/ZS1-128/master-layout-viewport-3.1.21/review.md'),\n"
p.write_text(''.join(lines))
# Refresh via the newly loaded generator so the repair entry reflects this publication.
import importlib
importlib.reload(g).main()
print(json.dumps(dict(public=str(Q),manifest=meta(Q/'manifest.json'),rust=171,pty=21)))

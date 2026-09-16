from pathlib import Path
import hashlib,json,re,shutil,sys,tarfile
R=Path(__file__).resolve().parents[3];D=Path(__file__).resolve().parent
sys.path.insert(0,str(R/'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g
Q=R/'Docs/quality/stage1/ZS1-104/master-summary-deferred-3.1.21'
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,d):p.write_text(json.dumps(d,ensure_ascii=False,indent=2)+'\n')
def read(p):return json.loads(p.read_text())
assert not Q.exists()
inputs=read(D/'root-after-inputs.json');assert len(inputs)==202 and {p:meta(R/p) for p in inputs}==inputs
result=read(D/'root-after-result.json');assert result['passed'] and meta(R/'target/debug/zenpi')==result['binary']
segments=re.split(r'^\s*Running tests/',(D/'root-after-regression.log').read_text(),flags=re.M)[1:]
counts={}
for segment in segments:
 name=segment.split('.rs',1)[0]
 rows=re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;',segment)
 assert rows,name
 counts[name]=dict(passed=int(rows[-1][0]),ignored=int(rows[-1][1]))
assert len(counts)==6 and sum(c['passed'] for c in counts.values())==118 and sum(c['ignored'] for c in counts.values())==4
for n in ['root-after-regression','root-after-format','root-after-clippy','root-after-build']:
 assert read(D/(n+'.run.json'))['exit_code']==0
assert read(D/'root-before.run.json')['exit_code']==101
http=[]
for phase,expected in [('before',False),('after',True)]:
 for field in ['finish_reason','stop_reason','stopReason','status']:
  directory=D/(phase+'-http-evidence')/field
  observation=read(directory/'observation.json')
  assert observation['rejected']==expected and observation['checkpoint_preserved']==expected and len(observation['requests'])==2
  if expected:assert 'invalid semantic summary' in observation['error']
  events=[json.loads(line).get('event',{}) for line in (directory/'journal.jsonl').read_text().splitlines()]
  totals={k:sum(e.get('type')==k for e in events) for k in ['semantic_checkpoint','semantic_summary_requested','semantic_summary_response']}
  assert totals==dict(semantic_checkpoint=1 if expected else 2,semantic_summary_requested=2,semantic_summary_response=2)
  http.append(dict(phase=phase,field=field,requests=2,counts=totals))
bp=v.parse((R/v.BLUEPRINT).read_text());assert sum(x.state=='[x]' for x in bp.items.values())==52
assert bp.items['ZS1-104'].state=='[ ]' and bp.items['ZS1-073'].state=='[x]'
Q.mkdir(parents=True)
for n in ['review.md','root.patch','rollback.patch','initial-inputs.json','root-before-inputs.json','root-after-inputs.json','root-before.log','root-before.run.json','root-before.py','root-after.py','root-after-regression.log','root-after-regression.run.json','root-after-format.log','root-after-format.run.json','root-after-clippy.log','root-after-clippy.run.json','root-after-build.log','root-after-build.run.json','root-after-result.json','publish.py']:
 shutil.copyfile(D/n,Q/n)
for n in ['before-http-evidence','after-http-evidence']:shutil.copytree(D/n,Q/n)
with tarfile.open(Q/'root-package.tar.gz','x:gz') as archive:
 for p in sorted(D.rglob('*')):
  if p.is_file() and 'root-bin' not in p.relative_to(D).parts:archive.add(p,arcname=str(p.relative_to(D)),recursive=False)
validation=v.validate(R,item='ZS1-073');assert validation['ok'],validation
dump(Q/'authority-validation.json',validation)
summary=dict(result,scope='104 local deferred summary repair only; full104 pending',rust_passed=118,suites=counts,new_http_negative_cases=http,before_top_level_failed=1,before_control_passed=1,review=str((Q/'review.md').relative_to(R)))
dump(D/'master-verification.json',summary);dump(Q/'master-verification.json',summary)
dump(Q/'manifest.json',dict(item='ZS1-104',full_item_accepted=False,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
p=R/'tools/generate_stage1_gantt.py';lines=p.read_text().splitlines(True);matches=[i for i,s in enumerate(lines) if s.startswith("    'ZS1-104':")];assert len(matches)==1
lines[matches[0]]="    'ZS1-104': ('延迟摘要误提交已修复：实际HTTP四种deferred标志原误替换→拒绝并保留旧checkpoint，正常引用允许；118项顶层Rust通过，原手动输入修复保留，整项待验', 'summary-deferred-integration-3.1.21', 'Docs/quality/stage1/ZS1-104/master-summary-deferred-3.1.21/review.md'),\n"
p.write_text(''.join(lines))
import importlib
importlib.reload(g).main()
print(json.dumps(dict(public=str(Q),manifest=meta(Q/'manifest.json'),rust=118,negative_http_fields=4)))

"""Read-only Python structural audit. No product import/process/network/write."""
from pathlib import Path, PurePosixPath
import csv,difflib,hashlib,json,re,tarfile
import sys
ROOT=Path(sys.argv[1]).resolve()
assert hashlib.sha256((ROOT/'manifest.json').read_bytes()).hexdigest() == '175d88faad7789422029f2a0b85c140f3a9248f260cff3aa115e2a26faef9dc1'
MAIN=Path('/Users/wangweiyang/GitHub/zenpi')
OWNED='Docs/learn/stage1_pi_mono/targets/zenpi/files/src/extensions.rs_learn.md'
REQ='3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d'
SNAP='3c01628b900c9078f7c1771f3638cc366b2e295c6fede7cfea191aa0620c5c2d'
SOURCE={'bytes':20742,'sha256':'38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed'}
BASE={'bytes':23071,'sha256':'9b1879edcfb70e711960092e12060a9137e2aacda6bdc547318e5ccb7510fe39'}
TEST={'bytes':6302,'sha256':'00a3873257e010a9fdd337d8b913fda3f2efe101710c45ae225975ab59c2237d'}
checks=[]
def check(label,ok):
 checks.append({'check':label,'passed':bool(ok)})
 if not ok:raise AssertionError(label)
def ident(b):return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def agrees(b,e):return ident(b)=={k:e[k] for k in ['bytes','sha256']}
def js(p):return json.loads(p.read_text())
def safe(root,rel):
 p=PurePosixPath(rel)
 if p.is_absolute() or '..' in p.parts or '\\' in rel:raise ValueError('unsafe path')
 q=root/rel
 if q.is_symlink() or not q.resolve().is_relative_to(root.resolve()):raise ValueError('unsafe link')
 return q

def archive_agrees(p,expected):
 with tarfile.open(p,'r:gz') as t:
  members=[m for m in t.getmembers() if m.isfile()]
  ok=len(members)==len(expected) and len({m.name for m in members})==len(members) and {m.name for m in members}==set(expected)
  for m in members:
   with t.extractfile(m) as f:b=f.read()
   ok=ok and agrees(b,expected[m.name])
 return ok

def main():
 before=(ROOT/'manifest.json').read_bytes();m=json.loads(before);files=m['files'];paths=[e['path'] for e in files]
 check('unique complete frozen payload inventory',len(paths)==len(set(paths)) and {p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*') if p.is_file()}==set(paths)|{'manifest.json'})
 check('all frozen payload identities',all(agrees(safe(ROOT,e['path']).read_bytes(),e) for e in files))
 check('one owned report candidate zero product executions',m['item_id']=='ZS1-080' and m['owned_paths']==[OWNED] and m['new_product_executions']==0 and m['status']=='worker candidate; master review pending')
 s=(ROOT/'target/src/extensions.rs').read_bytes();text=s.decode();capture=js(ROOT/'capture.json')
 check('current source20742B626lines exact identity',ident(s)==SOURCE and len(s.splitlines())==626 and agrees(s,capture))
 check('main source unchanged',(MAIN/'src/extensions.rs').read_bytes()==s)
 ledger=js(ROOT/'reading-ledger.json');cs=ledger['chunks'];pos=0;line=1;parts=[];ok=len(cs)==3
 for order,c in enumerate(cs,1):
  b=safe(ROOT,c['path']).read_bytes();ok=ok and c['read'] is True and c['order']==order and c['byte_start']==pos and c['line_start']==line and agrees(b,c) and b==s[pos:c['byte_end']] and len(b.splitlines())==c['line_end']-c['line_start']+1
  parts.append(b);pos=c['byte_end'];line=c['line_end']+1
 check('three actual contiguous reads cover every source byte and line',ok and pos==20742 and line==627 and b''.join(parts)==s)
 check('zero embedded tests accurately recorded',ledger['source_full_read_complete'] is True and ledger['embedded_test_count']==0 and s.count(b'#[test]')==0 and ledger['new_product_executions']==0)
 rows=list(csv.DictReader((ROOT/'chunk_manifest.tsv').read_text().splitlines(),delimiter='\t'))
 check('chunk TSV equals completed ledger',len(rows)==3 and all(all(str(c[k])==r[k] for k in r) for c,r in zip(cs,rows)))
 check('initial unread capture and plans preserved',capture['source_read_complete'] is False and all(e['read'] is False for n in ['chunk-plan.json','context-plan.json','history-read-plan.json'] for e in js(ROOT/n)))
 es=ledger['context_reads'];ok=len(es)==32
 for e in es:
  b=safe(ROOT,e['path']).read_bytes();original=Path(e['original']).read_bytes();ok=ok and e['read'] is True and agrees(b,e) and agrees(original,e['whole_identity']) and b''.join(original.splitlines(keepends=True)[e['line_start']-1:e['line_end']])==b
 check('32 exact main and upstream context excerpts',ok)
 tests=(ROOT/'context/full/tests/extensions.rs').read_bytes();ts=js(ROOT/'test-source-index.json');tt=tests.decode();lines=tt.splitlines()
 check('full180line companion test identity and coverage',ident(tests)==TEST and agrees(tests,ts) and len(lines)==180 and (MAIN/'tests/extensions.rs').read_bytes()==tests and safe(ROOT,es[0]['path']).read_bytes()==tests and ledger['external_test_source_full_read_complete'] is True)
 names=re.findall(r'#\[test\]\s*fn\s+(\w+)',tt);ds=ts['declarations']
 check('five ordinary tests four Unix cases zero ignored exact index',names==[d['name'] for d in ds] and len(names)==ts['test_declarations']==ts['ordinary_tests']==5 and ts['ignored_helpers']==0 and sum(d['unix_only'] for d in ds)==4 and all(d['read'] is True and d['new_executions']==0 and lines[d['function_line']-1].strip().startswith('fn '+d['name']+'(') for d in ds))
 maps=js(ROOT/'semantic-map.json');line=1;ok=True
 for e in maps:ok=ok and e['line_start']==line and e['line_end']>=line and e['read'] is True;line=e['line_end']+1
 check('semantic ranges cover source once through EOF',ok and line==627)
 defs=[{'line':i,'declaration':x.strip()} for i,x in enumerate(text.splitlines(),1) if re.match(r'\s*(pub(\([^)]*\))? )?(async )?(fn|struct|enum|const|type|static) ',x)]
 check('definition index matches all actual declarations',defs==js(ROOT/'definition-index.json'))
 sr=list(csv.DictReader((ROOT/'source_manifest.tsv').read_text().splitlines(),delimiter='\t'))
 check('understand source manifest maps one source to only owned report',len(sr)==1 and sr[0]['source_id']=='ZS1-080' and sr[0]['source_path']=='src/extensions.rs' and sr[0]['source_hash']==SOURCE['sha256'] and sr[0]['target_artifact']==OWNED and sr[0]['mapping_mode']=='understand' and sr[0]['status']=='[_]')
 docs=ledger['historical_documents'];ok=len(docs)==21
 for e in docs:
  b=safe(ROOT,e['path']).read_bytes();ok=ok and e['read'] is True and e['new_executions']==0 and agrees(b,e)
  if 'archive' in e:
   with tarfile.open(safe(ROOT,e['archive']),'r:gz') as t:
    with t.extractfile(e['member']) as f:ok=ok and f.read()==b
 check('21 historical document log receipt reads include exact archive views',ok)
 priors=js(ROOT/'prior-identities.json');ok=len(priors)==2
 for prior in priors:
  d=safe(ROOT,prior['copy']);original=Path(prior['original']);old=js(d/'manifest.json');ok=ok and agrees((d/'manifest.json').read_bytes(),prior['manifest']) and {p.relative_to(d).as_posix() for p in d.rglob('*') if p.is_file()}=={e['path'] for e in prior['files']}
  ok=ok and all(agrees(safe(d,e['path']).read_bytes(),e) and agrees(safe(original,e['path']).read_bytes(),e) for e in prior['files'])
  payload=d/'files' if old.get('schema_version')=='zenpi-single-file-review-entry/v1' else d
  ok=ok and all(agrees(safe(payload,e['path']).read_bytes(),e) for e in old['files'])
 check('complete old08019 and startup72 files unchanged including failures',ok and sorted(len(p['files']) for p in priors)==[19,72])
 oldroot=ROOT/'prior/target080-review-3.1.20-ready/files/Docs/quality/stage1/ZS1-080/worker-target-review-3.1.20'
 baseline=(ROOT/'target/baseline-extensions.rs').read_bytes();historical=(ROOT/'target/historical-extensions.rs').read_bytes()
 check('23071B685line frozen baseline distinct from current',ident(baseline)==BASE and len(baseline.splitlines())==685 and (oldroot/'baseline-extensions.rs').read_bytes()==baseline)
 check('current equals preserved historical source and companion tests',historical==s and (oldroot/'current-extensions.rs').read_bytes()==s and (oldroot/'tests__extensions.rs').read_bytes()==tests)
 for label,b in [('baseline',baseline),('historical',historical)]:check(label+' exact diff retained',(ROOT/(label+'-to-current.diff')).read_text()==''.join(difflib.unified_diff(b.decode().splitlines(True),text.splitlines(True),fromfile=label+'/src/extensions.rs',tofile='current/src/extensions.rs')))
 inventory=js(oldroot/'historical-evidence-index.json')['archives'];count=0;ok=True
 for name,e in inventory.items():
  p=safe(oldroot,name);count+=len(e['members']);ok=ok and agrees(p.read_bytes(),e) and archive_agrees(p,e['members'])
 check('103 nested historical archive members verified without extraction or execution',ok and count==103)
 startup=ROOT/'prior/extension-resume-startup-audit-ready';inputs=js(startup/'inputs.json');expected={e['path']:e for e in inputs}
 check('216 historical startup build inputs preserved without execution',len(inputs)==len(expected)==216 and archive_agrees(startup/'inputs.tar.gz',expected))
 rs=js(ROOT/'context-receipts.json');ok=[r['item_id'] for r in rs]==['ZS1-001'];count=0
 for r in rs:
  b=safe(ROOT,r['copy']).read_bytes();receipt=json.loads(b);ok=ok and agrees(b,r) and Path(r['original']).read_bytes()==b and receipt['complete'] is True and receipt['requirement_digest']==REQ and receipt['manual_review']['decision']=='accepted' and len(receipt['artifacts'])==len(r['artifacts'])
  for declared,saved in zip(receipt['artifacts'],r['artifacts']):
   count+=1;b=safe(ROOT,saved['copy']).read_bytes();ok=ok and declared==saved['declared'] and agrees(b,declared) and agrees(b,saved['observed']) and (MAIN/declared['path']).read_bytes()==b
 check('formal001 accepted receipt plus34 exact artifact references',ok and count==34)
 frozen=js(ROOT/'authority/Docs/execution/active_requirement.json');live=js(MAIN/'Docs/execution/active_requirement.json');assignment=js(ROOT/'assignment.json')
 check('authority3.1.21 requirement and080 baseline unchanged',frozen['requirement_digest']==live['requirement_digest']==REQ and live['blueprint_version']=='3.1.21' and all(live['baseline_files']['ZS1-080'][k]==v for k,v in BASE.items()))
 check('assignment53 snapshot and zero execution limits retained',assignment['completed']==53 and assignment['snapshot']==m['assignment_snapshot']==SNAP and assignment['allowed_new_product_executions']==0 and assignment['max_offline_audit_invocations']==1)
 report=(ROOT/'learn-report.md').read_text();base=(ROOT/'receiver-base.md').read_bytes()
 check('owned main report absent and receiver base empty',not (MAIN/OWNED).exists() and capture['canonical_exists'] is False and base==b'')
 check('forward patch creates exactly owned report',(ROOT/'forward.patch').read_text()==''.join(difflib.unified_diff([],report.splitlines(True),fromfile='/dev/null',tofile='b/'+OWNED)))
 check('reverse patch deletes exactly owned report',(ROOT/'reverse.patch').read_text()==''.join(difflib.unified_diff(report.splitlines(True),[],fromfile='a/'+OWNED,tofile='/dev/null')))
 check('report distinguishes unrun current criteria and preserved historical failures',all(x in report for x in [SOURCE['sha256'],BASE['sha256'],TEST['sha256'],'源内测试声明为 0','全部未运行','历史60 passed','历史116 passed','fixture never escaped','CommandTimeout(1000)','未执行条件']))
 protected=js(ROOT/'protected-identities.json');check('protected manifests117 fixtures and077 external audit logs unchanged',all(agrees(Path(e['path']).read_bytes(),e) for e in protected))
 for name,count in [('target070-full-3.1.21-20260913-ready',293),('target072-full-3.1.21-20260913-ready',225),('target073-full-3.1.21-20260913-ready',282),('target074-full-3.1.21-20260913-ready',125),('target077-full-3.1.21-20260913-ready',158)]:
  d=next(Path(e['path']).parent for e in protected if name in e['path']);old=js(d/'manifest.json');check(name+' full frozen payload unchanged',len(old['files'])==count and all(agrees(safe(d,e['path']).read_bytes(),e) for e in old['files']))
 check('94 preexisting non.ops worker files unchanged',len(capture['worker_status'])==len(capture['worker_files'])==94 and all(agrees(Path(e['path']).read_bytes(),e) for e in capture['worker_files']))
 check('reading corrections and route logged without product execution',js(ROOT/'reading-notes.json')['new_product_executions']==0 and js(ROOT/'route-decision.json')['operator_override']['new_child_tasks']==0 and js(ROOT/'looper-log.json')['instrument_policy_changed'] is False)
 check('frozen manifest unchanged by read-only auditor',(ROOT/'manifest.json').read_bytes()==before)
 return {'passed':len(checks),'failed':0,'payload_files':len(files),'source_bytes':20742,'source_lines':626,'external_test_declarations':5,'new_product_executions':0,'acceptance':'master independent semantic review pending'}
try:result=main()
except Exception as e:
 print(json.dumps({'ok':False,'checks':checks,'error':str(e)},ensure_ascii=False,indent=2));raise
else:print(json.dumps({'ok':True,'checks':checks,'result':result},ensure_ascii=False,indent=2))

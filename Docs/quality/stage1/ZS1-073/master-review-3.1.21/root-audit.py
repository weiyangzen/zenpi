"""Read-only structural audit. No product imports, execution, network or writes."""
from pathlib import Path, PurePosixPath
import csv
import difflib
import gzip
import hashlib
import json
import tarfile

import sys
ROOT=Path(sys.argv[1]).resolve(strict=True)
assert hashlib.sha256((ROOT/'manifest.json').read_bytes()).hexdigest() == '63f737ad072cd8454571c2306c79b540820d19751531cdaabb19bd019cedd5c1'
MAIN=Path('/Users/wangweiyang/GitHub/zenpi')
OWNED='Docs/learn/stage1_pi_mono/targets/zenpi/files/src/context.rs_learn.md'
REQ='3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d'
SUBJECT={'bytes':30013,'sha256':'256d9ac1657e272ba61ebfbc242ece2970ed0607f3ebb1583b1711f18b7bf229'}
BASELINE={'bytes':8199,'sha256':'bc4ce7999a0f4d162effece8c6f839aca3a230e97675a1d7908fca5d6a7fe909'}
checks=[]
def check(label,valid):
    checks.append({'check':label,'passed':bool(valid)})
    if not valid: raise AssertionError(label)
def identity(data):
    return {'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
def agrees(data,e):
    return identity(data)=={k:e[k] for k in ['bytes','sha256']}
def js(p): return json.loads(p.read_text())
def safe(root,rel):
    p=PurePosixPath(rel)
    if p.is_absolute() or '..' in p.parts or '\\' in rel: raise ValueError('unsafe path')
    q=root/rel
    if q.is_symlink() or not q.resolve().is_relative_to(root.resolve()): raise ValueError('unsafe link')
    return q

def main():
    before=(ROOT/'manifest.json').read_bytes();m=json.loads(before);files=m['files'];paths=[f['path'] for f in files]
    check('unique complete payload inventory',len(paths)==len(set(paths)) and {p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*') if p.is_file()}==set(paths)|{'manifest.json'})
    check('all frozen payload identities',all(agrees(safe(ROOT,f['path']).read_bytes(),f) for f in files))
    check('one owned report candidate with zero product runs',m['owned_paths']==[OWNED] and m['status']=='worker candidate; master review pending' and m['new_product_executions']==0)
    source=(ROOT/'target/src/context.rs').read_bytes()
    check('807 line current subject exact identity',identity(source)==SUBJECT and len(source.splitlines())==807)
    check('main subject remains captured bytes',(MAIN/'src/context.rs').read_bytes()==source)
    ledger=js(ROOT/'reading-ledger.json');cs=ledger['chunks'];ok=len(cs)==3;pos=0;line=1;parts=[]
    for order,c in enumerate(cs,1):
        b=safe(ROOT,c['path']).read_bytes();ok=ok and c['read'] is True and c['order']==order and c['line_start']==line and c['byte_start']==pos and agrees(b,c) and source[pos:c['byte_end']]==b and len(b.splitlines())==c['line_end']-c['line_start']+1
        pos=c['byte_end'];line=c['line_end']+1;parts.append(b)
    check('three actual continuous reads cover all source bytes',ok and pos==30013 and line==808 and b''.join(parts)==source)
    check('completed ledger excludes invented embedded tests or runs',ledger['source_full_read_complete'] is True and ledger['embedded_test_count']==0 and ledger['new_product_executions']==0 and b'#[test]' not in source and b'#[cfg(test)]' not in source)
    rows=list(csv.DictReader((ROOT/'chunk_manifest.tsv').read_text().splitlines(),delimiter='\t'))
    check('TSV and actual source reads agree',len(rows)==3 and all(all(str(c[k])==r[k] for k in r) for c,r in zip(cs,rows)))
    check('initial unread capture and plans remain historical',js(ROOT/'capture.json')['source_read_complete'] is False and all(c['read'] is False for c in js(ROOT/'chunk-plan.json')) and all(c['read'] is False for c in js(ROOT/'context-plan.json')))
    ok=len(ledger['context_reads'])==31
    for e in ledger['context_reads']:
        b=safe(ROOT,e['path']).read_bytes();original=Path(e['original']).read_bytes()
        ok=ok and e['read'] is True and agrees(b,e) and agrees(original,e['whole_identity']) and b''.join(original.splitlines(keepends=True)[e['line_start']-1:e['line_end']])==b
    check('31 exact current bounded and full-test read segments',ok)
    ts=js(ROOT/'test-source-index.json');ok=len(ts)==3
    for t in ts:
        b=safe(ROOT,t['path']).read_bytes();es=[e for e in ledger['context_reads'] if e['original'].endswith('/'+t['path'].split('context/full/')[1])]
        ok=ok and agrees(b,t) and b''.join(safe(ROOT,e['path']).read_bytes() for e in es)==b and len(b.splitlines())==t['lines'] and t['new_executions']==0 and b.count(b'#[test]')==len(t['test_declarations'])
    check('all1497 external test lines and28 declarations only, not executions',ok and sum(t['lines'] for t in ts)==1497 and sum(len(t['test_declarations']) for t in ts)==28 and sum(t['ignored_helpers'] for t in ts)==3)
    check('14 historical documents actually read retain identity',len(ledger['historical_documents'])==14 and all(e['read'] is True and agrees(safe(ROOT,e['path']).read_bytes(),e) for e in ledger['historical_documents']))
    priors=js(ROOT/'prior-identities.json');ok=len(priors)==3
    for prior in priors:
        base=safe(ROOT,prior['copy']);original=Path(prior['original'])
        ok=ok and agrees((base/'manifest.json').read_bytes(),prior['manifest']) and {p.relative_to(base).as_posix() for p in base.rglob('*') if p.is_file()}=={f['path'] for f in prior['files']}
        ok=ok and all(agrees(safe(base,f['path']).read_bytes(),f) and agrees(safe(original,f['path']).read_bytes(),f) for f in prior['files'])
        old=js(base/'manifest.json');payload=base/'files' if original.name=='target073-review-3.1.20-ready' else base
        ok=ok and all(agrees(safe(payload,f['path']).read_bytes(),f) for f in old['files'])
        if 'patch' in old:ok=ok and agrees(safe(base,old['patch']['path']).read_bytes(),old['patch'])
    check('all three complete old073 packages unchanged including old manifests and scripts',ok)
    a=ROOT/'prior/target073-ready';b=ROOT/'prior/target073-review-3.1.20-ready';c=ROOT/'prior/target073-refresh-3.1.20-ready'
    check('frozen baseline retains both original and gzip identities',identity((ROOT/'target/baseline-context.rs').read_bytes())==BASELINE and identity(gzip.decompress((a/'baseline-context.rs.gz').read_bytes()))==BASELINE)
    inventory=js(a/'replay-input-inventory.json'); expected={f['path']:f for f in inventory}
    with tarfile.open(a/'replay-inputs.tar.gz','r:gz') as archive:
        members=[f for f in archive.getmembers() if f.isfile()]
        ok=len(members)==len(expected) and len({f.name for f in members})==len(members) and {f.name for f in members}==set(expected)
        for member in members:
            with archive.extractfile(member) as stream: data=stream.read()
            ok=ok and agrees(data,expected[member.name])
            if member.name in ['tests/context.rs','tests/stage1_context_checkpoint.rs']:
                ok=ok and data==(ROOT/'context/full'/member.name).read_bytes()
    check('139 historical archive inputs unchanged; two current test files equal old inputs',ok and len(expected)==139)
    check('historical source identical but separate new reading',gzip.decompress((a/'context.rs.gz').read_bytes())==source and (c/'source/current-context.rs').read_bytes()==source and (ROOT/'current-versus-prior.diff').read_bytes()==b'')
    prior_report=(b/'files'/OWNED).read_bytes();refresh=(c/'files'/OWNED).read_bytes()
    check('complete old report read assembly preserves exact prefix and appendix',refresh==prior_report+(c/'appendix.md').read_bytes() and prior_report.startswith((a/'learn-report.md').read_bytes()))
    check('baseline difference accurately generated',(ROOT/'baseline-to-current.diff').read_text()==''.join(difflib.unified_diff((ROOT/'target/baseline-context.rs').read_text().splitlines(True),source.decode().splitlines(True),fromfile='baseline/src/context.rs',tofile='current/src/context.rs')))
    rs=js(ROOT/'context-receipts.json');ok=[r['item_id'] for r in rs]==['ZS1-001','ZS1-012','ZS1-013'];count=0
    for r in rs:
        data=safe(ROOT,r['copy']).read_bytes();receipt=json.loads(data);ok=ok and agrees(data,r) and Path(r['original']).read_bytes()==data and receipt['complete'] is True and receipt['requirement_digest']==REQ and receipt['manual_review']['decision']=='accepted' and len(receipt['artifacts'])==len(r['artifacts'])
        for declared,saved in zip(receipt['artifacts'],r['artifacts']):
            count+=1;data=safe(ROOT,saved['copy']).read_bytes();ok=ok and declared==saved['declared'] and agrees(data,declared) and agrees(data,saved['observed']) and (MAIN/declared['path']).read_bytes()==data
    check('formal001 and source012013 receipts with123 exact artifact references',ok and count==123)
    authority=js(ROOT/'authority/Docs/execution/active_requirement.json');live=js(MAIN/'Docs/execution/active_requirement.json')
    check('requirement3.1.21 and073 baseline remain unchanged',authority['requirement_digest']==live['requirement_digest']==REQ and live['blueprint_version']=='3.1.21' and all(live['baseline_files']['ZS1-073'][k]==v for k,v in BASELINE.items()))
    base=(ROOT/'receiver-base.md').read_bytes();report=(ROOT/'learn-report.md').read_bytes()
    check('main owned baseline remains unmodified exact3036 bytes',(MAIN/OWNED).read_bytes()==base and len(base)==3036 and base==(a/'prior-report.md').read_bytes())
    forward=''.join(difflib.unified_diff(base.decode().splitlines(True),report.decode().splitlines(True),fromfile='a/'+OWNED,tofile='b/'+OWNED))
    reverse=''.join(difflib.unified_diff(report.decode().splitlines(True),base.decode().splitlines(True),fromfile='a/'+OWNED,tofile='b/'+OWNED))
    check('forward patch precisely updates only owned report',(ROOT/'forward.patch').read_text()==forward)
    check('reverse patch precisely restores captured receiver bytes',(ROOT/'reverse.patch').read_text()==reverse)
    check('report distinguishes estimates historical evidence and unrun criteria',all(s in report.decode() for s in [SUBJECT['sha256'],'全部未运行','25 个普通测试声明','123 个 artifact','临时内存','reported tokens']))
    protected=js(ROOT/'protected-identities.json')
    check('protected manifests and117 fixtures remain unchanged',all(agrees(Path(e['path']).read_bytes(),e) for e in protected))
    for item,count in [('target070-full-3.1.21-20260913-ready',293),('target072-full-3.1.21-20260913-ready',225)]:
        directory=next(Path(e['path']).parent for e in protected if item in e['path']);old=js(directory/'manifest.json')
        check(item+' all payloads including defect evidence unchanged',len(old['files'])==count and all(agrees(safe(directory,f['path']).read_bytes(),f) for f in old['files']))
    captured=js(ROOT/'capture.json')
    check('94 existing non.ops worker files unchanged',len(captured['worker_status'])==94 and len(captured['worker_files'])==94 and all(agrees(Path(f['path']).read_bytes(),f) for f in captured['worker_files']))
    check('frozen manifest unchanged by read-only auditor',(ROOT/'manifest.json').read_bytes()==before)
    return {'passed':len(checks),'failed':0,'payload_files':len(files),'new_product_executions':0,'source_lines':807,'source_bytes':30013,'acceptance':'master independent review pending'}
try:
    result=main()
except Exception as e:
    print(json.dumps({'ok':False,'checks':checks,'error':str(e)},ensure_ascii=False,indent=2))
    raise
else:
    print(json.dumps({'ok':True,'checks':checks,'result':result},ensure_ascii=False,indent=2))

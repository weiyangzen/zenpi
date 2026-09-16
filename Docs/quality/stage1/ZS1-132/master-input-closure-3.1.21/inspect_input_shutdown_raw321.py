from pathlib import Path
import hashlib, json, shutil
R=Path(__file__).resolve().parents[2]
D=R/'.ops/stage1_execution/input-shutdown-current-3.1.21'
W=Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/stage132-input-ticket321')
Q=D/'raw-worker-observations'
assert not Q.exists();Q.mkdir()
def rows(p):return [json.loads(l) for l in p.read_text().splitlines()]
def js(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for name in ['before-02','after-01']:shutil.copytree(W/name,Q/name)
for name in ['baseline-binary-capture.json','candidate-binary-capture.json','ticket_host.rs','probe.py']:shutil.copy2(W/name,Q/name)
checks={};results=[]
def check(k,v):checks[k]=bool(v)
for revision in ['before-02','after-01']:
    root=Q/revision;summary=js(root/'result.json')
    expected_binary=js(Q/'baseline-binary-capture.json')['sha256'] if revision=='before-02' else js(Q/'candidate-binary-capture.json')['ticket-host-after']['sha256']
    check(revision+'-binary-binding',summary['binary_sha256']==expected_binary)
    check(revision+'-five-cases',len(summary['cases'])==5 and len(summary['hosts'])==10)
    for case in summary['cases']:
        name=case['name'];d=root/name;prefix=revision+'/'+name
        wire=rows(d/'first/stdout.jsonl');reconnect=rows(d/'reconnect/stdout.jsonl')
        sent=js(d/'first/sent.json');resent=js(d/'reconnect/sent.json')
        context=next(v['project'] for v in wire if v.get('id')=='initial')
        requests=[v for v in sent if v['type']=='input_queue']
        check(prefix+'-same-request-fingerprints',all(v in resent for v in requests))
        check(prefix+'-initial-identity',context==case['context'])
        for label in ['first','reconnect']:
            meta=js(d/label/'meta.json')
            check(prefix+'-'+label+'-natural-reaped',meta['exit_code']==0 and meta['reaped'] and meta['readers_joined'] and not meta.get('forced_cleanup',False))
        before_wal=rows(d/'wal-before-reconnect.jsonl');after_wal=rows(d/'home/session.jsonl.reconnect')
        before_queue=[v for v in rows(d/'journal-before-reconnect.jsonl') if v.get('event',{}).get('type')=='input_queue']
        after_queue=[v for v in rows(d/'home/session.jsonl') if v.get('event',{}).get('type')=='input_queue']
        check(prefix+'-reconnect-queue-events-exact',before_queue==after_queue)
        check(prefix+'-wal-prefix-preserved',after_wal[:len(before_wal)]==before_wal)
        stop=next((v for v in wire if v.get('id')=='stop' and v.get('type')=='response'),None)
        if stop is not None:
            check(prefix+'-ack-last-and-drained',wire[-1]==stop and stop['data']=={'closing':True,'drained':True})
            check(prefix+'-ack-original-identity',stop['project']==context)
            check(prefix+'-ack-wal-stable',sha(d/'wal-at-ack.jsonl')==sha(d/'wal-before-reconnect.jsonl'))
        for req in requests:
            rid=req['id'];k=prefix+'/'+rid
            terminals=[v for v in wire if v.get('type')=='response' and v.get('id')==rid]
            cached=[v for v in reconnect if v.get('type')=='response' and v.get('id')==rid]
            historical=[v for v in before_wal if v.get('event',{}).get('id')==rid]
            final=[v for v in after_wal if v.get('event',{}).get('id')==rid]
            check(k+'-ticket-wal-no-rewrite-or-append',historical==final)
            missing=revision=='before-02' and case['mode']=='delayed'
            check(k+'-expected-terminal-count',len(terminals)==(0 if missing else 1))
            if missing:
                check(k+'-old-unknown-only-after-reconnect',len(cached)==1 and cached[0]['code']=='unknown_outcome' and not any(v['event']['type']=='terminal' for v in historical))
            else:
                terminal=terminals[0]
                check(k+'-original-project-session',terminal['project']==context and req['session_id']==context['session_id'])
                check(k+'-exact-cached-reconnect',cached==[terminal])
                stored=[v['event']['line'] for v in historical if v['event']['type']=='terminal']
                check(k+'-exact-one-durable-terminal',len(stored)==1 and json.loads(stored[0])==terminal)
                if stop is not None:check(k+'-terminal-before-ack',wire.index(terminal)<wire.index(stop))
                if case['mode']=='delayed':
                    check(k+'-truthful-unknown',terminal['code']=='input_queue_result_unknown' and not terminal['success'] and 'may have been applied' in terminal['error'] and 'inspect the input queue' in terminal['error'])
        results.append(dict(revision=revision,case=name,ticket_count=len(requests),queue_events_before=len(before_queue),queue_events_after=len(after_queue),wal_rows_before=len(before_wal),wal_rows_after=len(after_wal)))
out=dict(passed=all(checks.values()),checks=checks,cases=results,scope='independent read of retained finite worker raw observations; no worker probe execution, no root runtime claim, no forced in-service mutation case')
(D/'root-raw-review.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')
(D/'raw-observation-manifest.json').write_text(json.dumps({p.relative_to(Q).as_posix():dict(bytes=p.stat().st_size,sha256=sha(p)) for p in Q.rglob('*') if p.is_file()},indent=2)+'\n')
print(json.dumps(dict(passed=out['passed'],checks=len(checks),failed=[k for k,v in checks.items() if not v],cases=results)))
assert out['passed']

import hashlib,json,pathlib
root=pathlib.Path(__file__).resolve().parent
work=root.parents[3]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
read=lambda p:json.loads(p.read_text())
lines=lambda p:[json.loads(l) for l in p.read_text().splitlines()]
binding=read(root/'binary-binding.json');assert sha(pathlib.Path(binding['path']))==binding['sha256']
guards={r['path']:r['compiled_sha256'] for r in read(root/'included-guards.json')}
for r in read(root/'captured-main-inputs.json')['files']:
    assert sha(work/'.ops/provider-kill319-current'/r['path'])==guards.get(r['path'],r['sha256']),r['path']
source_hashes={sha(root/'kill_probe.py'),sha(root/'kill_probe-v1.py')}
passed=[]
for provider,case in [('chat','partial-first'),('chat','tool-corrected'),('anthropic','partial-first'),('anthropic','tool-first'),('google','partial-first'),('google','tool-first')]:
    p=root/provider/case;result=read(p/'result.json');assert result['passed'] and result['binary_sha256']==binding['sha256'] and result['harness_sha256'] in source_hashes
    for r in read(p/'artifact-index.json'): assert sha(p/r['path'])==r['sha256'],r['path']
    owners=[read(p/f'owner-{n}.process.json')['pid'] for n in ('one','two','three')];assert len(set(owners))==3
    assert read(p/'owner-one.exit.json')['returncode']==-9
    assert all(read(p/f'owner-{n}.exit.json')['returncode']==0 for n in ('two','three'))
    http=read(p/'http-server-result.json');assert not http['errors'] and http['held_connection_closed'];assert http['requests']==(2 if case.startswith('partial') else 3)
    wal=lines(p/'after-kill.session.jsonl.reconnect');stored={r['event']['sequence']:r['event']['line'] for r in wal if r['event']['type']=='event'}
    for r in wal:
        data={k:v for k,v in r.items() if k!='sha256'}
        assert hashlib.sha256(json.dumps(data,ensure_ascii=False,separators=(',',':'),sort_keys=True).encode()).hexdigest()==r['sha256']
    public=(p/'owner-one.stdout.jsonl').read_text().splitlines(True)
    assert all(stored[json.loads(raw)['sequence']]==raw for raw in public)
    j=lines(p/'after-kill.session.jsonl');assert not any('INTERRUPTED_PARTIAL_319' in json.dumps(r.get('turn')) for r in j)
    original=[r['event'] for r in j if r.get('event',{}).get('operation_kind')=='provider'];assert len(original)==1
    op=read(p/'recovery-inspect.json')['data']['operations'][0];assert op['operation_id']==original[0]['operation_id'] and op['idempotency_key']==original[0]['idempotency_key']
    if case.startswith('tool'):
        first=read(p/'http-request-1.json')['body'];last=read(p/'http-request-2.json')['body']
        if provider=='chat':
            calls=[m for m in last['messages'] if m['role']=='assistant' and m.get('tool_calls')];results=[m for m in last['messages'] if m['role']=='tool']
            assert len(calls)==len(results)==1 and calls[0]['tool_calls'][0]['id']==results[0]['tool_call_id']=='durable_read_319'
            assert calls==[m for m in first['messages'] if m['role']=='assistant']
        elif provider=='anthropic':
            calls=[m for m in last['messages'] if m['role']=='assistant'];assert calls==[m for m in first['messages'] if m['role']=='assistant'] and len(calls)==1
            results=[b for m in last['messages'] for b in m['content'] if b['type']=='tool_result'];assert len(results)==1 and results[0]['tool_use_id']=='durable_read_319'
        else:
            calls=[m for m in last['contents'] if m['role']=='model'];assert calls==[m for m in first['contents'] if m['role']=='model'] and len(calls)==1
            results=[b['functionResponse'] for m in last['contents'] for b in m['parts'] if 'functionResponse' in b];assert len(results)==1 and results[0]['id']=='durable_read_319'
    passed.append(f'{provider}/{case}')
print(json.dumps({'passed_cases':passed,'snapshot_files_verified':len(read(root/'captured-main-inputs.json')['files']),'binary_sha256':binding['sha256'],'result':'pass'}))

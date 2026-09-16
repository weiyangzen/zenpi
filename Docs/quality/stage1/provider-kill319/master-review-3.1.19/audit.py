from pathlib import Path
import json,hashlib
root=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
lines=lambda p:[json.loads(l) for l in p.read_text().splitlines()]
binary=read(root/'binary-binding.json');assert sha(Path(binary['path']))==binary['sha256']
repo=root.parents[2]
assert all(sha(repo/p)==h for p,h in read(root/'source-inputs.json').items())
passed=[];pids=[]
for provider in ['chat','anthropic','google']:
    run=read(root/(provider+'.run.json'));assert run['exit_code']==0 and sha(root/(provider+'.log'))==run['sha256']
    for scenario in ['partial','tool']:
        p=root/'actual'/provider/(scenario+'-master');result=read(p/'result.json')
        assert result['passed'] and result['binary_sha256']==binary['sha256'] and result['harness_sha256']==sha(root/'kill_probe.py')
        for x in read(p/'artifact-index.json'):
            data=(p/x['path']).read_bytes();assert len(data)==x['bytes'] and hashlib.sha256(data).hexdigest()==x['sha256']
        owners=[read(p/f'owner-{n}.process.json') for n in ['one','two','three']]
        assert len({x['pid'] for x in owners})==3
        assert all(x['binary_sha256']==binary['sha256'] and x['HOME_preserved'] and x['CODEX_HOME_preserved'] for x in owners)
        assert read(p/'owner-one.exit.json')['returncode']==-9
        assert all(read(p/f'owner-{n}.exit.json')['returncode']==0 for n in ['two','three'])
        pids.extend(x['pid'] for x in owners)
        http=read(p/'http-server-result.json');assert http['held_connection_closed'] and not http['errors']
        assert http['requests']==(2 if scenario=='partial' else 3)
        wal=lines(p/'after-kill.session.jsonl.reconnect')
        for row in wal:
            data=json.dumps(row['event'],ensure_ascii=False,separators=(',',':'),sort_keys=True).encode()
            assert hashlib.sha256(data).hexdigest()==row['sha256']
        stored={row['event']['sequence']:row['event']['line'] for row in wal if row['event']['type']=='event'}
        public=(p/'owner-one.stdout.jsonl').read_text().splitlines(True)
        assert all(stored[json.loads(raw)['sequence']]==raw for raw in public)
        replay={row['sequence']:raw for raw in (p/'owner-two.stdout.jsonl').read_text().splitlines(True) if (row:=json.loads(raw)).get('type')=='event'}
        assert all(replay[json.loads(raw)['sequence']]==raw for raw in public)
        journal=lines(p/'after-kill.session.jsonl');assert not any('INTERRUPTED_PARTIAL_319' in json.dumps(row.get('turn')) for row in journal)
        started=[row['event'] for row in journal if row.get('event',{}).get('operation_kind')=='provider'];assert len(started)==1
        op=read(p/'recovery-inspect.json')['data']['operations'][0];assert op['operation_id']==started[0]['operation_id'] and op['idempotency_key']==started[0]['idempotency_key']
        out2=[(raw,json.loads(raw)) for raw in (p/'owner-two.stdout.jsonl').read_text().splitlines(True)]
        out3=[(raw,json.loads(raw)) for raw in (p/'owner-three.stdout.jsonl').read_text().splitlines(True)]
        terminal=[raw for raw,value in out2 if value.get('id')=='fresh' and value.get('success') is True]
        terminal3=[raw for raw,value in out3 if value.get('id')=='fresh' and value.get('success') is True]
        assert len(terminal)==2 and terminal[0]==terminal[1] and terminal3==terminal[:1]
        if scenario=='tool':
            first=read(p/'http-request-1.json')['body'];last=read(p/'http-request-2.json')['body']
            if provider=='chat':
                calls=[m for m in last['messages'] if m['role']=='assistant' and m.get('tool_calls')];results=[m for m in last['messages'] if m['role']=='tool'];assert len(calls)==len(results)==1 and calls[0]['tool_calls'][0]['id']==results[0]['tool_call_id']=='durable_read_319';assert calls==[m for m in first['messages'] if m['role']=='assistant']
            elif provider=='anthropic':
                calls=[m for m in last['messages'] if m['role']=='assistant'];assert len(calls)==1 and calls==[m for m in first['messages'] if m['role']=='assistant'];results=[b for m in last['messages'] for b in m['content'] if b['type']=='tool_result'];assert len(results)==1 and results[0]['tool_use_id']=='durable_read_319'
            else:
                calls=[m for m in last['contents'] if m['role']=='model'];assert len(calls)==1 and calls==[m for m in first['contents'] if m['role']=='model'];results=[b['functionResponse'] for m in last['contents'] for b in m['parts'] if 'functionResponse' in b];assert len(results)==1 and results[0]['id']=='durable_read_319'
        passed.append(dict(provider=provider,scenario=scenario,processes=[x['pid'] for x in owners],http_requests=http['requests'],public_events=len(public),wal_records=len(wal),passed=True))
assert len(set(pids))==18
(root/'audit-result.json').write_text(json.dumps(dict(cases=passed,binary_sha256=binary['sha256'],processes=18,all_current_source_hashes_match=True,release_build=False),indent=2)+'\n')
print('PASS: six current-main native SSE SIGKILL/recovery scenarios;18 independent CLI processes; byte-exact WAL/replays and native tool identities checked')

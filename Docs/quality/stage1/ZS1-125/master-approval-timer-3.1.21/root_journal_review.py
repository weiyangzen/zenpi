"""Inspect the already recorded PTY journal; no product process is executed."""
from pathlib import Path
import collections, hashlib, json
D=Path(__file__).resolve().parent
P=D/'root-pty-evidence'
raw=(P/'workspace/session.jsonl').read_bytes()
entries=[json.loads(line) for line in raw.decode().splitlines()]
counts=collections.Counter(row['event']['type'] if row['kind']=='event' else row['kind'] for row in entries)
turns=[row['turn'] for row in entries if row['kind']=='turn']
assert len(turns)==1 and turns[0]['metadata']['origin']=='user_shell'
result=json.loads(turns[0]['content'].split('\n',1)[1])
assert result['execution_started'] and result['child_reaped'] and result['exit_code']==0
assert result['stdout']=='TIMER-GATE-COMPLETED\n'
events={row['event']['type']:row for row in entries if row['kind']=='event'}
assert events['approval_resolved']['seq'] < events['operation_started']['seq'] < events['tool_execution_started']['seq'] < events['tool_execution_finished']['seq']
assert events['approval_resolved']['event']['decision']=='allow'
assert events['operation_finished']['event']['outcome']=='succeeded'
assert len({events[k]['event']['operation_id'] for k in ['operation_started','tool_execution_started','tool_execution_finished','operation_finished']})==1
pty=json.loads((P/'result.json').read_text())
assert len(pty['checks'])==13 and all(c['ok'] for c in pty['checks'][:12])
assert pty['checks'][12]['name']=='one local shell journal turn' and not pty['checks'][12]['ok']
assert pty['exit']==0 and pty['error'] is None and pty['model_requests']==0 and pty['gate_requests']==1
out={'passed':True,'executes_product':False,'source':str(P/'workspace/session.jsonl'),'source_sha256':hashlib.sha256(raw).hexdigest(),'source_bytes':len(raw),'corrected_counts':dict(counts),'local_shell_result':{'origin':turns[0]['metadata']['origin'],'exit_code':result['exit_code'],'child_reaped':result['child_reaped'],'stdout':result['stdout']},'original_pty_driver_exit':1,'product_exit':0,'original_pty_behavior_checks_passed':12,'original_extra_journal_check_passed':False,'explanation':'The extra counter read event.type for every row, but session and turn use top-level kind. The original run/check/log remain unchanged. This separate offline review reads all original JSONL rows and verifies the local user-shell turn and approval-before-execution order; no second PTY or model call.'}
print(json.dumps(out,ensure_ascii=False,indent=2))

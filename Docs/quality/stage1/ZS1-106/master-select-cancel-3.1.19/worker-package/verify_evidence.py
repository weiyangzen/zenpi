#!/usr/bin/env python3
import json,hashlib,argparse
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('evidence',type=Path);a=p.parse_args();r=a.evidence
load=lambda f:json.loads((r/f).read_text());rows=lambda b:[json.loads(x) for x in b.splitlines()]
result=load('result.json');assert result['passed'] is True
assert load('cancel-response.json')['data']['cancel_requested'] is True
selection=load('select-response.json');assert selection['success'] is False and 'cancel' in json.dumps(selection).lower()
before=(r/'pre-cli-cancel.jsonl').read_bytes();after=(r/'post-cli-cancel.jsonl').read_bytes();assert after.startswith(before);added=rows(after)[len(rows(before)):];assert not any('turn' in x or x.get('event',{}).get('type')=='session_tree' for x in added)
assert load('after-cancel-leaf.json')['data']['active_leaf']==result['original_leaf']
for folder in ['before-select','after-cancel','after-real-continuation']:
 assert (r/folder/'effect-count.txt').read_bytes()==b'EXECUTED\n'
 c=load(folder+'/project-tabs.json');v=next(x for x in c['project_state'] if x['name']==c['projects'][c['active']]);m=json.dumps(v['messages']);assert 'COMMON_A_REAL' in m and 'BRANCH_C_EFFECT' in m and 'BRANCH_B_ONLY' not in m
final=rows((r/'after-real-continuation/journal.jsonl').read_bytes());assert sum(x.get('turn',{}).get('role')=='tool' for x in final)==1;assert sum(x.get('event',{}).get('type')=='tool_execution_finished' for x in final)==1
reqs=load('http-requests.json');assert len(reqs)==5;last=reqs[-1]['body'];wire=json.dumps(last);assert 'COMMON_A_REAL' in wire and 'COMMON_A_FIXTURE' in wire and 'BRANCH_C_EFFECT' in wire and 'BRANCH_B_ONLY' not in wire;assert last['messages'][-1]['content']=='AFTER_CANCEL_CONTINUE';assert sum(m.get('role')=='tool' for m in last['messages'])==1
for n in [0,1]:assert b'\x1b[?1049l' in (r/f'terminal-{n}.ansi').read_bytes()
cli=load('cancel-cli/process.json');assert cli['returncode']==0 and cli['normal_shutdown']
stream=[json.loads(x) for x in (r/'cancel-cli/stdout.jsonl').read_text().splitlines()];assert next(x for x in stream if x.get('type')=='response' and x.get('id')=='cancel-this-select')==selection
events=[x['event']['type'] for x in stream if x.get('type')=='event' and x.get('request_id')=='cancel-this-select'];assert events==['request_accepted','request_started','cancel_requested'],events
print(json.dumps({'passed':True,'compiled_tui_processes':2,'compiled_cli_processes':1,'actual_http_requests':5,'actual_effect_count':1,'cancelled_selection_response':selection,'journal_prefix_unchanged':True,'new_tree_actions_during_cancel':0,'scope':'offline verification only; does not run product'},indent=2))

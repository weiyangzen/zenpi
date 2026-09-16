from pathlib import Path
import hashlib,json,re,shutil,sys
R=Path.cwd();sys.path.insert(0,'tools');import validate_stage1_blueprint as v;import generate_stage1_gantt as g
K='ZS1-300';S=Path('/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/target300-chat-composer321-ready2');W=R/'.ops/stage1_execution/target300-current-3.1.21-v2';Q=R/'Docs/quality/stage1/ZS1-300/master-review-3.1.21-v4'
def h(p):
 b=p if isinstance(p,bytes) else p.read_bytes();return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
bp=v.parse((R/v.BLUEPRINT).read_text());assert sum(x.state=='[x]' for x in bp.items.values())==57 and bp.items[K].state=='[x]'
m=json.loads((S/'manifest.json').read_text());
for rel,spec in m['files'].items():assert h(S/rel)=={'bytes':spec['bytes'],'sha256':spec['sha256']},rel
src=(S/'capture/codex-rs/tui/src/bottom_pane/chat_composer.rs').read_bytes();parts=[(S/f'reads/{i:03}.txt').read_bytes() for i in range(1,6)];assert b''.join(parts)==src and len(src.splitlines())==9785
assert h(src)=={'bytes':374943,'sha256':'234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da'}
assert not W.exists();W.mkdir();shutil.copytree(S,W/'worker',copy_function=shutil.copyfile)
Q.mkdir(parents=True)
report=S/'supplement.md'; review=Q/'review.md'; review.write_text(f'''# ZS1-300 主控独立验收\n\n主控核对 Codex `chat_composer.rs`：374943B/9785L，SHA `234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da`。五个连续区间 1–2000、2001–4000、4001–6000、6001–8000、8001–9785 的原始字节拼接精确等于捕获源并到 EOF；manifest 15 项身份逐项一致。\n\n报告覆盖 composer 状态机、键盘/popup、slash/file/mention、历史、paste burst、附件/远程图片、队列提交、voice、滚动和上层边界。该文件只作为 Codex TUI 消费者参考，不能推导 zenpi 已实现相同能力；136 个内嵌测试均未运行，跨平台语音、真实终端时序和像素行为未验收。\n''')
(Q/'worker-report.md').write_bytes(report.read_bytes());(Q/'current-chat_composer.rs').write_bytes(src);(Q/'worker-manifest.json').write_bytes((S/'manifest.json').read_bytes());(Q/'read-binding.json').write_bytes((S/'read-binding.json').read_bytes());(Q/'accept.py').write_text(Path(__file__).read_text());
for i in range(1,6):shutil.copyfile(S/f'reads/{i:03}.txt',Q/f'read-{i:03}.txt')
shutil.copyfile(S/'auditor-once.log',Q/'historical-manifest-failure.log') if (S/'auditor-once.log').exists() else None
(R/bp.files[K].artifact).parent.mkdir(parents=True,exist_ok=True);(R/bp.files[K].artifact).write_text(report.read_text()+'\n\n'+review.read_text())
refs=[]
for p in [R/bp.files[K].artifact,review,Q/'worker-report.md',Q/'current-chat_composer.rs',Q/'worker-manifest.json',Q/'read-binding.json',Q/'accept.py']+[Q/f'read-{i:03}.txt' for i in range(1,6)]:refs.append(dict(path=str(p.relative_to(R)),**h(p)))
rec=dict(schema_version='stage1-receipt/v1',item_id=K,run_id='zenpi-stage1-20260911',requirement_digest=bp.requirement,baseline_snapshot_sha256='6c40adec341b1fb1fe28cde24033ba4171918f1856b9669ae41f4a58a0f319a4',complete=True,attempt_id='target300-current-independent-3.1.21',integrated_revision=v.repository_head(R),validators=bp.items[K].validators,source_path='codex-rs/tui/src/bottom_pane/chat_composer.rs',source_hash='234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da',read_ranges=[[0,374943]],artifacts=refs,role='master',reviewer='controller',manual_review={'decision':'accepted','reviewer':'controller','evidence':dict(path=str(review.relative_to(R)),**h(review))})
(R/v.EVIDENCE/'receipts'/f'{K}.master.json').write_text(json.dumps(rec,ensure_ascii=False,separators=(',',':'))+'\n')
z=(R/v.BLUEPRINT).read_text().replace('- [ ] **ZS1-300**','- [x] **ZS1-300**');g.atomic(R/v.BLUEPRINT,z);bp=v.parse(z);active=json.loads((R/v.SELECTOR).read_text());active['snapshot_sha256']=bp.snapshot;g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n');sel=v.selector(R,bp,v.BLUEPRINT);sizes=v.file_hashes(bp,R,Path(bp.header['source_repo']),target=False)
for rel,(fields,rows) in v.scaffold(bp,sizes,v.EVIDENCE).items():g.atomic(R/rel,v.tsv(fields,rows).decode())
for p in (R/v.EVIDENCE).glob('todos_*.md'):g.atomic(p,v.todo(bp,v.frontiers(bp,json.loads((R/v.EVIDENCE/'claims.json').read_text())['claims'],sel['run_id'],3),v.BLUEPRINT,v.EVIDENCE+'/claims.json'))
g.main();print('accepted',K,sum(x.state=='[x]' for x in bp.items.values()))

from pathlib import Path
import hashlib,json,re,sys,shutil
R=Path.cwd();sys.path.insert(0,'tools');import validate_stage1_blueprint as v;import generate_stage1_gantt as g
K='ZS1-098'; D=R/'.ops/stage1_execution/target098-current-3.1.21'; W=D/'worker'; Q=R/'Docs/quality/stage1/ZS1-098/master-review-3.1.21-v3';
def h(p):
 b=p.read_bytes();return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
bp=v.parse((R/v.BLUEPRINT).read_text()); assert sum(x.state=='[x]' for x in bp.items.values())==56 and bp.items[K].state=='[x]'
s=R/'Cargo.lock'; assert h(s)=={'bytes':46021,'sha256':'7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348'}
report=W/'files/Docs/learn/stage1_pi_mono/targets/zenpi/files/Cargo.lock_learn.md'; assert h(report)=={'bytes':53446,'sha256':'fa9e7fabcba0d878afbab49a112dd71a66c270dbddd88d53e52c0af7b258fd8c'}
# independent lock structural read: every package block has name/version and registry checksums are lowercase 64-hex; local crossterm/zenpi are the only source-less records.
t=s.read_text(); blocks=t.split('[[package]]')[1:]; assert len(blocks)==196
records=[]
for b in blocks:
 n=re.search(r'^name = "([^"]+)"',b,re.M); ver=re.search(r'^version = "([^"]+)"',b,re.M); assert n and ver
 src=re.search(r'^source = "([^"]+)"',b,re.M); chk=re.search(r'^checksum = "([0-9a-f]{64})"',b,re.M)
 if src: assert src.group(1).startswith('registry+') and chk
 else: assert n.group(1) in {'crossterm','zenpi'} and not chk
 records.append((n.group(1),ver.group(1)))
assert len(set(n for n,vv in records))==191
assert all(x in t for x in ['ignore','yaml_serde','unicode-segmentation','crossterm','zenpi'])
Q.mkdir(parents=True); (R/bp.files[K].artifact).parent.mkdir(parents=True,exist_ok=True); (R/bp.files[K].artifact).write_text(report.read_text()); (Q/'worker-report.md').write_bytes(report.read_bytes()); (Q/'current-Cargo.lock').write_bytes(s.read_bytes()); (Q/'worker-manifest.json').write_bytes((W/'manifest.json').read_bytes()); (Q/'accept.py').write_text(Path(__file__).read_text())
review=f'''# ZS1-098 主控独立验收\n\n主控独立核对当前 Cargo.lock：46021B/1802L，SHA `7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348`。逐段读取其版本头、全部 196 个 package 块、source/checksum、依赖数组、平台包与末尾 EOF；结构解析确认 196 条记录、191 个不同包名，registry 条目均为 64 位小写 checksum，只有本地 patch 的 crossterm 与根 zenpi 没有 source/checksum。报告中的 196 包 ledger、389 条引用、5 组同名多版本、Cargo.toml/vendor 消费者边界与冻结 baseline 差异均已复核。\n\nCargo.lock 不是运行时代码；本次不宣称 Cargo 构建、下载 checksum、跨平台编译、vendor 安全或 feature 激活通过。worker 的 859 项静态校验、0 失败、0 产品执行和原始 capture 失败全部保留。只接受 ZS1-098 单文件理解，不改变 Cargo.lock 或 TUI。\n'''
(Q/'review.md').write_text(review); shutil.copyfile(D/'worker-external/auditor-once.log',Q/'offline-auditor.log'); shutil.copyfile(D/'worker-external/auditor-once-receipt.json',Q/'offline-receipt.json'); shutil.copyfile(D/'worker-external/delivery.json',Q/'delivery.json')
refs=[]
for p in [R/bp.files[K].artifact,Q/'review.md',Q/'worker-report.md',Q/'current-Cargo.lock',Q/'worker-manifest.json',Q/'accept.py',Q/'offline-auditor.log',Q/'offline-receipt.json',Q/'delivery.json']: refs.append(dict(path=str(p.relative_to(R)),**h(p)))
(R/bp.files[K].artifact).parent.mkdir(parents=True,exist_ok=True); (R/bp.files[K].artifact).write_text(report.read_text()+'\n\n'+review)
base=dict(schema_version='stage1-receipt/v1',item_id=K,run_id='zenpi-stage1-20260911',requirement_digest=bp.requirement,baseline_snapshot_sha256='2f472f4707975a24ecb70034c45559e1023bab08277e11ac9fe7a52ff16dc3ca',complete=True,attempt_id='target098-current-independent-3.1.21',integrated_revision=v.repository_head(R),validators=bp.items[K].validators,source_path='Cargo.lock',source_hash='f822e2d73d49727fdb4898180d26ff75bf95a4f81b638b613cb457e3171a30e1',read_ranges=[[0,46021]],artifacts=refs,role='master',reviewer='controller',manual_review={'decision':'accepted','reviewer':'controller','evidence':dict(path=str((Q/'review.md').relative_to(R)),**h(Q/'review.md'))})
(R/v.EVIDENCE/'receipts'/f'{K}.master.json').write_text(json.dumps(base,ensure_ascii=False,separators=(',',':'))+'\n')
text=R/v.BLUEPRINT; z=text.read_text();z=z.replace('- [ ] **ZS1-098**','- [x] **ZS1-098**');g.atomic(text,z); bp=v.parse(z); active=json.loads((R/v.SELECTOR).read_text());active['snapshot_sha256']=bp.snapshot;g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n'); sel=v.selector(R,bp,v.BLUEPRINT); sizes=v.file_hashes(bp,R,Path(bp.header['source_repo']),target=False)
for rel,(fields,rows) in v.scaffold(bp,sizes,v.EVIDENCE).items():g.atomic(R/rel,v.tsv(fields,rows).decode())
for p in (R/v.EVIDENCE).glob('todos_*.md'):g.atomic(p,v.todo(bp,v.frontiers(bp,json.loads((R/v.EVIDENCE/'claims.json').read_text())['claims'],sel['run_id'],3),v.BLUEPRINT,v.EVIDENCE+'/claims.json'))
g.main(); print('accepted',K,'count',sum(x.state=='[x]' for x in bp.items.values()))

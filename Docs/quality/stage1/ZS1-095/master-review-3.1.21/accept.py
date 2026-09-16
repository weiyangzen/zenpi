from pathlib import Path
import datetime, difflib, hashlib, json, re, shutil, subprocess, sys, tarfile

R = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(R / 'tools'))
import validate_stage1_blueprint as v
import generate_stage1_gantt as g

KEY = 'ZS1-095'
D = R / '.ops/stage1_execution/target095-current-3.1.21'
W = D / 'worker'
Q = R / 'Docs/quality/stage1/ZS1-095/master-review-3.1.21'
assert not Q.exists()

def meta(p):
    b = p.read_bytes()
    return dict(bytes=len(b), sha256=hashlib.sha256(b).hexdigest())

def read(p):
    return json.loads(p.read_text())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

def rec(p):
    return dict(path=str(p.relative_to(R)), **meta(p))

bp = v.parse((R / v.BLUEPRINT).read_text())
sel = v.selector(R, bp, v.BLUEPRINT)
f = bp.files[KEY]
assert bp.requirement == '3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d'
assert sum(i.state == '[x]' for i in bp.items.values()) == 49
assert bp.items[KEY].state == '[ ]'
assert all(bp.items[k].state == '[x]' for k in bp.items[KEY].depends)
assert not (R / f.artifact).exists()
for role in ['worker', 'master']:
    assert not (R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json').exists()
assert meta(W / 'manifest.json')['sha256'] == 'f81de305a9af883b5b847811edd93083d9ca2f2cdc89e97584ae9174e57fa712'
assert meta(W / 'files' / f.artifact)['sha256'] == '4bd75736d235e90feda9394a9601bf4da991be6c8463ca0ae29634de7fab2ab8'
source = R / f.path
assert meta(source)['sha256'] == '5488ad365670e515eba5e7d82a91dbf935be854b848c20fd99725f102e1f185f'
assert source.read_bytes() == (W / 'capture/zenpi/src/approval.rs').read_bytes()
baseline = W / 'history/zs1-095-ready/evidence/source/baseline-approval.rs'
assert meta(baseline) == dict(bytes=27606, sha256=f.sha256)
old = baseline.read_bytes().splitlines(keepends=True)
new = source.read_bytes().splitlines(keepends=True)
assert len(old) == 737 and len(new) == 780
# The frozen source is entirely present in the independently read current file.
# Preserve exact matching blocks; do not claim a separate second full reading.
blocks = difflib.SequenceMatcher(a=old, b=new, autojunk=False).get_opcodes()
assert all(tag in ('equal', 'insert') for tag, *_ in blocks)
assert sum(a1-a0 for tag,a0,a1,b0,b1 in blocks if tag == 'equal') == 737
assert sum(b1-b0 for tag,a0,a1,b0,b1 in blocks if tag == 'insert') == 43
run = read(D / 'root-offline/run.json')
assert run['exit_code'] == 0
audit = [json.loads(line) for line in (D / 'root-offline/stdout.log').read_text().splitlines()]
assert audit[-1] == dict(checks=159, failed=0, structural_only=True, runtime_execution=False)
assert all(row['passed'] for row in audit[:-1])

review = '''## 主控独立逐文件验收 — ZS1-095 / 3.1.21

主控顺序完整读取当前 src/approval.rs 的 L1–260、261–520、521–780（29396 B，SHA-256 5488ad365670e515eba5e7d82a91dbf935be854b848c20fd99725f102e1f185f），并完整读取本次 37582 B / 213 行候选报告和 80 行离线 verifier。前两块在前一执行轮读取，最后一块及候选在本验收轮完成；没有把摘要、manifest 或函数数量当作阅读。当前源码包含冻结 c592686334a95047f749d7c61c793abc03ce93d5312b22aadeefa5aed9c85844 的全部 737 行，仅插入 43 行 / 1790 B 的会话记忆恢复；本轮直接检查精确差异并保存匹配区间。基线不改写，也不虚报再次全文读旧源码。

本文件的请求校验、serde 默认、四个共享集合、29 个函数（23 个生产函数和 6 个源测试）、策略优先级、两类错误与所有条件分支均已复核。六个源测试实际包含 14 个普通 assert 文本位置、0 快照；本轮没有执行这些测试。worker 的 31 连续语义单元和 16 个功能映射对照完整报告，分别关联具体状态、源测试/可运行判据及实际 owner。结构计数仅证明覆盖材料完整。

核心结论：默认策略 Always 不等于默认全拒绝，普通只读在显式 Deny 和 worker/preflight 分支之后才被允许；TUI 默认选中 Deny。请求只做通用结构校验，不认证 policy digest/lease，不校验工具专有 schema 或自动脱敏。协调器共享 Arc，pending 按键排序且只 drain 一次，没有本模块容量或 TTL。respond 不以可见性为前置；首响应只在当前 pending/decision 生命周期内获胜。accepted 是内存记录，mark_persisted 不做 IO，也不阻止等待者提前得到 response。persist 回调在锁外，失败撤回不一定恢复 pending；并发 persist 可重复调用外部 writer，mark 失败可能发生在外部写成功之后。这些 API 限制不是未经证明的宿主可达漏洞。

取消也有独立语义：cancel_all 只拒绝仍 pending 的请求，不改已接受 Allow；emergency_cancel 增加 epoch，真正停下依赖调用方闭包读取 epoch，无法撤销已发生副作用。取消闭包在锁内执行，不应重入协调器。retract 在 decision 已被消费时返回 Unknown；未显示请求没有恢复快照，有显示快照的恢复仍保留 visible，不能承诺再次 drain。accepted 尚存时 ID 可重新登记；当前 core 派生 ID 依赖 session/turn/call/policy 生命周期，协调器自身没有永久消费墓碑。

主控新读 core L2590–2768、4240–4450、3300–3317、5311–5335，确认实际 user_shell 在成功 append 审批事件、按需 remember、拒绝/取消/gate 检查、begin_operation 和 tool_execution_started 之后才调用真实 RunCommandTool。普通工具 prepare 路径在 persist_accepted、remember、approval_consumed、Deny 和取消/binding 检查之后才继续执行准备；这不是本轮完整审阅其余 dispatch 或底层 fsync/崩溃恢复。富响应可先于落盘返回与 core 的落盘后执行顺序不矛盾。读 core L1246–1287、1338–1363、1654–1687、5188–5215，确认独立配置基准、会话记忆恢复和同步 reveal/respond 两锁接口。配置 Deny 受恢复筛选保护；remember 本身无 guard；恢复事件需要可信 session 来源，不能把 origin 字符串当认证。

主控新读 headless L5630–5710、6335–6408、4505–4598、7207–7248，确认原子响应 ack 只表示 accepted；project owner 协调器由宿主选择；EOF 普通拒绝和显式 shutdown epoch 取消各有用途。本轮未将 headless 缓存刷新或全部跨项目并发路径宣告已穷尽。

主控新读当前 e451 TUI L1051–1184、1180–1295、1344–1387、12214–12246、12601–12644：项目/request/turn/call 校验后才响应，y/n 选择后 Enter 提交，Esc 保留，worker 不提供 remember，目录选择器优先；失败响应只显示错误，不把错误当决策退役，下一轮协调器状态负责 reconciliation。视图的 128 容量不等于协调器容量。全部 tests/tui_approval_focus.rs 428 行已读（12 个上下文测试），另读 tests/approval_owner.rs L1–203、385–400；静态断言分别覆盖默认拒绝、草稿、owner、重复、记忆重放和取消。没有把 TestBackend 当 PTY，也没有把 executes_once 名称当故障下恰好一次证明。

worker 捕获时主控 125 仍在运行的语句保留为历史时间点。当前已完成 125 局部计时/footer 整合，主控记录在 Docs/quality/stage1/ZS1-125/master-approval-timer-3.1.21/review.md：195 项 Rust 与新 debug 真实 PTY 14 项通过，原失败未删除。本项不重复运行或合并那些测试。Codex 303 对照沿用 worker 独立源审阅；主控本轮没有重读整份 303 或宣告它已验收。旧 095 完整包及旧失败只作保留历史，不继承旧运行数作为当前证明。

主控完整静态读取新的 portable verifier 后，在新的主控副本仅运行一次，159 项离线检查全部通过，0 产品运行。它只核文件绑定、patch/rollback、完整包和临时目录文档操作，不运行 Cargo、PTY、HTTP、旧 runner、release 或预算。该结果支撑交付完整性，接受依据仍是上述源码和调用路径审阅。

仅接受 095 的完整文件理解（G-FILE），保留冻结身份与当前实现身份。审批完整交互 124、计时/长回复 125、生命周期 132、core/headless/TUI 整文件、相关目录及全阶段各自待验。回退仅撤销本项报告/receipt 状态，产品源码、BentoBox、顶部加号目录选择行为与历史证据不变。
'''

Q.mkdir(parents=True)
(Q / 'review.md').write_text(review)
shutil.copyfile(W / 'files' / f.artifact, Q / 'worker-report.md')
shutil.copyfile(W / 'manifest.json', Q / 'worker-manifest.json')
shutil.copyfile(source, Q / 'current-approval.rs')
shutil.copyfile(baseline, Q / 'baseline-approval.rs')
shutil.copyfile(Path(__file__), Q / 'accept.py')
for n in ['stdout.log', 'stderr.log', 'run.json']:
    shutil.copyfile(D / 'root-offline' / n, Q / ('offline-' + n))
dump(Q / 'read-binding.json', dict(
    formal_source=rec(Q / 'current-approval.rs'), current_line_ranges=[[1,260],[261,520],[521,780]],
    current_byte_ranges=[[0,sum(map(len,new[:260]))],[sum(map(len,new[:260])),sum(map(len,new[:520]))],[sum(map(len,new[:520])),sum(map(len,new))]],
    baseline=rec(Q / 'baseline-approval.rs'), baseline_to_current_opcodes=blocks,
    baseline_full_text_reused_from_current=True, separate_baseline_full_reread=False,
    report=rec(Q / 'worker-report.md'), product_executions=0))
with tarfile.open(Q / 'worker-ready.tar.gz', 'x:gz') as archive:
    for p in sorted(W.rglob('*')):
        if p.is_file():
            archive.add(p, arcname=str(p.relative_to(W)), recursive=False)
sizes = v.file_hashes(bp, R, Path(bp.header['source_repo']), target=False)
for rel in [v.BLUEPRINT, v.SELECTOR, v.EVIDENCE + '/claims.json', *v.scaffold(bp, sizes, v.EVIDENCE)]:
    dest = Q / 'authority-before' / rel
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(R / rel, dest)
(R / f.artifact).parent.mkdir(parents=True, exist_ok=True)
g.atomic(R / f.artifact, (W / 'files' / f.artifact).read_text() + '\n\n' + review)
refs = [rec(R / f.artifact)] + [rec(Q / n) for n in [
    'review.md','worker-report.md','worker-manifest.json','current-approval.rs',
    'baseline-approval.rs','read-binding.json','accept.py','worker-ready.tar.gz',
    'offline-stdout.log','offline-run.json']]
base = dict(schema_version='stage1-receipt/v1', item_id=KEY, run_id=sel['run_id'],
    requirement_digest=bp.requirement, baseline_snapshot_sha256=sel['baseline_snapshot_sha256'],
    complete=True, attempt_id='target095-current-independent-3.1.21',
    integrated_revision=v.repository_head(R), validators=bp.items[KEY].validators,
    source_path=f.path, source_hash=f.sha256, read_ranges=[[0,27606]], artifacts=refs)
for role in ['worker','master']:
    receipt = dict(base, role=role, reviewer='controller' if role == 'master' else 'worker C with controller current qualification')
    if role == 'master':
        receipt['manual_review'] = dict(decision='accepted', reviewer='controller', evidence=rec(Q/'review.md'),
            findings='Complete current780-line source and213-line report independently reviewed; frozen737 lines exact-mapped, all coordinator/policy/error/test branches and bounded real consumers reviewed.159 new offline checks pass,0 product executions; only095 file understanding accepted.')
    g.atomic(R / v.EVIDENCE / 'receipts' / f'{KEY}.{role}.json', json.dumps(receipt,ensure_ascii=False,separators=(',',':'))+'\n')

def state(mark):
    text, count = re.subn(r'^- \[[ _x]\]( \*\*ZS1-095\*\*)', '- '+mark+r'\1', (R/v.BLUEPRINT).read_text(), flags=re.M)
    assert count == 1
    updated = v.parse(text)
    assert updated.requirement == bp.requirement
    g.atomic(R/v.BLUEPRINT,text)
    active = read(R/v.SELECTOR)
    active['snapshot_sha256'] = updated.snapshot
    g.atomic(R/v.SELECTOR,json.dumps(active,ensure_ascii=False,separators=(',',':'))+'\n')
    for rel,(fields,rows) in v.scaffold(updated,sizes,v.EVIDENCE).items():
        g.atomic(R/rel,v.tsv(fields,rows).decode())
    front = v.frontiers(updated,read(R/v.EVIDENCE/'claims.json')['claims'],sel['run_id'],3)
    for p in (R/v.EVIDENCE).glob('todos_*.md'):
        g.atomic(p,v.todo(updated,front,v.BLUEPRINT,v.EVIDENCE+'/claims.json'))

state('[_]')
pre = v.validate(R,item=KEY)
dump(Q/'before-promotion.json',pre)
assert pre['ok'],pre
state('[x]')
post = v.validate(R,item=KEY)
dump(Q/'after-promotion.json',post)
assert post['ok'],post
argv = ['python3','tools/validate_stage1_blueprint.py','--item',KEY]
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
result = subprocess.run(argv,cwd=R,capture_output=True,timeout=60)
(Q/'gstage.stdout.log').write_bytes(result.stdout)
(Q/'gstage.stderr.log').write_bytes(result.stderr)
dump(Q/'gstage.run.json',dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=result.returncode,stdout=meta(Q/'gstage.stdout.log'),stderr=meta(Q/'gstage.stderr.log')))
assert result.returncode == 0,result.stderr
dump(Q/'manifest.json',dict(item=KEY,master_accepted=True,artifacts={str(p.relative_to(Q)):meta(p) for p in Q.rglob('*') if p.is_file()}))
g.main()
print(json.dumps(dict(accepted=KEY,counts=post['counts'],snapshot=post['snapshot_sha256'])))

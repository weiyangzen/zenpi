from pathlib import Path
import json, hashlib, difflib, shutil, datetime, os
W=Path(__file__).resolve().parent; C=W/'checkout'; R=W.parent/'zs1-125-tail-wiring-ready'
assert not R.exists()
def sha(b): return hashlib.sha256(b).hexdigest()
def rec(b): return {'bytes':len(b),'sha256':sha(b)}
def put(rel,b):
 p=R/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b)
def js(rel,v): put(rel,(json.dumps(v,indent=2)+'\n').encode())
paths=['src/tui.rs','tests/tui_transcript_ux.rs'];frozen=json.loads((W/'frozen-inputs.json').read_text())
final=json.loads((W/'cargo-clippy-final-08-inputs.json').read_text())
assert set(final)==set(frozen['files'])
changes={p:{'before':frozen['files'][p],'after':final[p]} for p in final if frozen['files'][p]!=final[p]}
assert set(changes)==set(paths+['src/render.rs']),changes.keys()
assert final['src/render.rs']['sha256']=='b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d'
for p in paths:
 assert (W/'before'/p).read_bytes()==(W/'before'/p).read_bytes()
 assert rec((C/p).read_bytes())==final[p]
 assert rec((W/'before'/p).read_bytes())==frozen['files'][p]
assert (C/paths[1]).read_bytes().startswith((W/'before'/paths[1]).read_bytes())
patch=''
for p in paths:
 a=(W/'before'/p).read_bytes();b=(C/p).read_bytes();put('before/'+p,a);put('after/'+p,b)
 patch+=''.join(difflib.unified_diff(a.decode().splitlines(True),b.decode().splitlines(True),fromfile='a/'+p,tofile='b/'+p))
put('product.patch',patch.encode())
for p in W.glob('*.json'):
 if (W/(p.stem+'.log')).exists():
  r=json.loads(p.read_text());put('commands/'+p.name,p.read_bytes());put('commands/'+r['log'],(W/r['log']).read_bytes());put('commands/'+r['inputs_file'],(W/r['inputs_file']).read_bytes())
for name in ['frozen-inputs.json','dependency.json','read-ledger.json','prior-preservation.json','helper-failure-notes.txt','source-review.md','failed-test-01.rs','failed-clippy-06.rs']:
 put('evidence/'+name,(W/name).read_bytes())
put('evidence/final-inputs.json',(W/'cargo-clippy-final-08-inputs.json').read_bytes())
for name in ['freeze.py','run.py','package.py']:
 put('scripts/'+name,(W/name).read_bytes())
js('evidence/scope.json',dict(captured_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),frozen_full_master_copy=str(C),frozen_input_files=len(final),product_paths=paths,actual_private_copy_changes=changes,prerequisite_render_excluded_from_patch=True,source_lines_read=14461,external_test_file_lines_read=372,new_tests=3,existing_transcript_tests=13,transcript_tests_passed=16,tui_unit_tests_passed=23,render_unit_tests_passed=23,render_integration_tests_passed=4,cargo_invocations=8,rustfmt_invocations=3,rustc_version_invocations=1,failed_cargo_invocations=['cargo-test-01','cargo-clippy-06'],full_product_context=True,whole_test_suite_claimed=False,substitute_harnesses=0,pty_runs=0,http_runs=0,master_writes=0,other_worker_writes=0,status_writes=0,complete=False))
readme=f'''# ZS1-125 tail wiring candidate

在新的私有完整主库快照中接线完成。仅 `src/tui.rs` 的 `transcript_window` 和 `tests/tui_transcript_ux.rs` 的3项追加测试进入 `product.patch`。测试完成不等于主库接受；ZS1-125完成状态、G-FILE/G-STAGE及与A的同文件冲突由master处理。

前置是不可变 `.ops/zs1-125-render-tail-ready`，manifest `822b201ac8e8f3f5bb4172599ac42c807addbba69c13362d24c26576d36f9581`；其render after hash `b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d`。本包不重发render文件或render补丁。依赖详情见evidence/dependency.json。master应先采用该前置，再合并本增量。

TUI before: {frozen['files']['src/tui.rs']['sha256']}，591434字节、14461行。
TUI after: {final['src/tui.rs']['sha256']}，{final['src/tui.rs']['bytes']}字节。
测试 before: {frozen['files']['tests/tui_transcript_ux.rs']['sha256']}，12405字节、372行。
测试 after: {final['tests/tui_transcript_ux.rs']['sha256']}，{final['tests/tui_transcript_ux.rs']['bytes']}字节。
product.patch: {sha(patch.encode())}。

两份source已分别连续全文阅读和解释，见evidence/source-review.md与read-ledger.json。冻结完整copy包含4096个主库tracked/untracked非ignored输入，冻结时逐文件重读hash确认无漂移；每个Rust命令另存完整输入清单及指纹。所有实际编译都在该copy进行，CARGO_TARGET_DIR独立，原HOME和CODEX_HOME保留，未创建替代模块或harness。

接线始终使用tail metadata API；position视觉行号等于omitted_visual_lines + local_index。窗口仍从最新消息倒序遍历，保留原消息id/message_offset、8192行和MAX_MARKDOWN_CELLS预算、省略标记以及BentoBox/经典render缓存和锚点逻辑。单消息裁剪也设置省略标志。正文、journal、复制源和项目状态不因显示裁剪删除。

原生命令均指定stable-aarch64-apple-darwin（rustc1.94.0，host aarch64-apple-darwin），Cargo全部--offline --locked，CARGO_NET_OFFLINE=true。最终检查：

- transcript UX: 16 passed，含13项原测试及3项新增。
- 完整库TUI单元测试过滤tui::: 23 passed。
- 完整库render单元测试: 23 passed。
- render_markdown集成: 4 passed。
- clippy --lib --test tui_transcript_ux --test render_markdown -- -D warnings: exit0。

共8次Cargo调用（6次test含1次编译失败，2次clippy含1次失败）、3次rustfmt、1次rustc版本检查。首轮新增测试误调用private方法，exit101，改为公开render。首次Clippy拒绝新增测试的2处needless borrow，exit101，去掉借用后重跑UX和Clippy通过。所有原始Cargo日志、命令/exit/输入hash/日志hash及两份失败测试源码均保留。vendor/crossterm已有unused_parens警告仍显示，但没有本crate告警。没有依赖阻塞，也没有执行整个仓库测试全集。TUI/render单元测试之后只修改集成测试借用；产品TUI/render字节始终保持已测hash。

新增测试覆盖9000行单消息的plain/Markdown/fenced code，8180行跨8192阈值、9000行后继续同消息追加，固定宽度下阅读锚点，End/latest，单物理长行CJK+组合字符在3/4/5/8/12列的尾部，以及回到BentoBox宽屏。原多消息18000行、滚动窗口、message_offset消息淘汰、项目往返、省略标记和窄屏恢复全部通过。

限制：精确视觉offset针对最多256KiB的被接受显示输入；超限上游或附加块仍受既有字节截头政策限制。宽度重排、修改旧前缀或Markdown重新解释可改变视觉行，不保证语义锚点；End可恢复新布局最新。已被窗口淘汰的锚点夹到最早保留行。预算仅剩1行时既有省略提示可占用唯一行；pane没有内容面积时无法显示正文。本次保留这些既有边界。

43份旧manifest及2313项字典artifact、168项列表artifact均逐字节hash验证，原ready不改写。helper-failure-notes记录一次历史manifest路径描述误当artifact的只读检查失败及修正。主库、A worktree、异步/session-browser、授权状态、任务和调度器均无写入。未运行PTY、HTTP或网络测试。

运行 `python3 verify.py` 只做离线完整性与临时git init中的精确before/after正反向check/apply/hash/sentinel回放，不执行Cargo、不改主库。验证收据存于worker的 `.ops/zs1-125-tail-wiring-work/offline-verification.json`。
'''
put('README.md',readme.encode())
print(R)

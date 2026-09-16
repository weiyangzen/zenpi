# ZS1-125 — 单文件尾部渲染 provisional 实现

本候选仅修改 `src/render.rs` 及其内联测试，未接线TUI、未改其它产品文件或主库/蓝图/索引/状态。它解决单条合法256KiB回复超过8192视觉行时最新尾部无法由head API取回的问题。仍待主控接线及真实入口验证，不宣称ZS1-125或096已完成。

## 文件理解与精确基线

主库当前文件和2267 worker起始文件逐字节相同：901行/30217B，SHA256 `6eaaa712745659ae48f429217c5eb8961dd12e71789f8a2bc70867ee71ba25a7`。按1–310、311–620、621–901连续完整阅读，包含所有block parser、inline parser、wrap/sanitize/prefix/profile常量以及11个内联测试；必要`tests/render_markdown.rs`全部4个测试另读并冻结。096候选路径在本worker不存在，没有将候选报告代替全文理解。before/src/render.rs为精确旧字节，baseline.json记录身份和读段。

原模块是轻量Markdown子集：段落/标题/围栏code/quote/list/rule，未知语法保留文本；围栏可在EOF未闭合。parse先保留head 256KiB，sanitize把tab展开并再次限制字节；head renderer顺序处理block并限制输出。prefixed API在body前加role/续行缩进；plain不解释Markdown。原wrap按Unicode标量宽度，rule按viewport生成字符；inline parser的未闭合link可能对每个`[`重新find整个剩余suffix，intraword underscore也曾反复扫描旧前缀。仅靠TUI滚动不可能从这些head返回值找回被丢弃的单消息尾部。

原公共head API签名与head选择策略保留；共享parser只是抽出有界输入入口，共享inline scanner保持既有成功link/underscore语法，改成单调搜索及常数前驱状态。其余旧函数仅有rustfmt格式调整。本轮没有把整个原head路径的所有既有瞬时分配或Unicode弱点一并改写。

## 新API与主控锚点接线

约定的两个Vec API均已提供：

- `render_markdown_tail_prefixed(prefix: &str, input: &str, width: usize, prefix_style: Style) -> Vec<Line<'static>>`
- `render_plain_tail_prefixed(prefix: &str, input: &str, width: usize, prefix_style: Style) -> Vec<Line<'static>>`

两者包装对应的`render_markdown_tail_prefixed_with_metadata` / `render_plain_tail_prefixed_with_metadata`，签名参数相同，返回公开`RenderedTail { lines: Vec<Line<'static>>, omitted_visual_lines: usize }`。`omitted_visual_lines + local_index`就是本次宽度/新renderer下的精确单消息视觉行索引；没有用伪造0隐藏裁剪。主控可以用 `(message_ordinal, omitted_visual_lines + local_index)`。总视觉行数可由`omitted_visual_lines + lines.len()`得到。普通流式追加保留下来的既有视觉行索引不会因窗口裁剪重置；测试按重叠原索引逐行比较。

计数仅针对接受的最多256KiB输入，不包含超额输入。超过256KiB仍采用既有head-byte截断且UTF-8边界安全，不承诺显示超额部分最新尾部。必须对同一消息始终使用新metadata renderer，不宜先用旧scalar-wrap索引、越过8192后才换成新grapheme索引。宽度/role缩进变更，或追加闭合Markdown使较早内容重解析，都可能改变视觉布局，主控须按其重排策略使旧锚点失效/重新定位；metadata不是跨任意语义编辑的永久ID。首个保留行重新显示role，但内部code/list marker只在原物理/视觉首行出现，裁掉的marker不会被伪造插回。

## 有界算法与上下文

先从最多256KiB原始输入生成不会膨胀的compact文本：CRLF/lone CR归一化、其它控制替换`?`，tab保留为紧凑token。沿原顺序解析完整有界block树，所以尾部在未闭合fence/inline span内部也保留原Markdown上下文；没有倒转源文本、按尾字节猜fence或将Markdown整体降级plain。

所有block先以共享换行状态机计数。计数回调不分配Line/Span/Rule字符串，rule计1；块间空行、code语言行和plain末尾空行都计入。随后从末block往前选择足够的行；code按原物理行逆向选取、每个物理行仍按正序换行。长段落的输出通过VecDeque逐行保留尾部，最多limit个完成行及一个进行中行，不会先构造全部视觉行再take。quote/list的长marker只在保留行装饰后分配，避免给大量将被丢弃的一格body行反复拷贝巨大缩进。宽rule只为最后选中的block分配，不先构造数万个65535列分隔线。

width夹1..65535，输出行数上限为`min(8192, floor(4Mi cells / full_width))`，full_width包含role前缀。这样正文+缩进+分隔线共同受cells预算约束。输入及各文本/inline副本为O(256KiB)，block/segment元数据数量由有界源字节数约束；换行队列和输出为O(行上限+cells)。tab只在换行时逐cell发出，最多源tab数×4的线性工作，不创建膨胀后截掉尾字节的巨型字符串。最终保留文本UTF-8字节可按`4 * MAX_MARKDOWN_CELLS + 2 * MAX_MARKDOWN_BYTES`保守上界解释（非零宽grapheme、生成ASCII cells以及输入/role中的零宽附加字节）；这不是整个进程RSS的实测限额或精确分配量。Vec/String容量及block/Span元数据有额外常数开销，本轮未声称测得内存峰值。

为控制实际终端列宽，新tail路径按extended grapheme计算，不将VS16、ZWJ或组合字符切成独立显示单元。跨Markdown样式边界的grapheme也先在有界文本上合并，整体采用首字节样式；这可能牺牲单个组合标记的独立样式，以换取不可分glyph的一致宽度。放不下的宽grapheme和行首独立零宽cluster显示`?`。role的换行展平成空格，其它控制被sanitize。旧head的标量wrap策略保持兼容，不把两套视觉索引当成完全一致。

## 病理扫描

LinkScan对`]`和`)`维护单调绝对位置，连EOF失败也缓存。同一未闭合/无效closer不会在后续每个`[`重复扫描；测试instrumented scanned_bytes在合法256KiB级别的多种未闭合输入族上不超过2N，且真实tail调用完成。保留现有首`]`、紧随`](`、首`)`和非空target语法，不尝试引入完整Markdown链接解析器。intraword underscore缓存最近非underscore字符，避免长标识符underscore后缀反复trim整个已读前缀。没有把测试耗时写成真实TUI帧预算通过。

## 实际验证与失败历史

所有命令使用native `cargo +stable-aarch64-apple-darwin`，独立`CARGO_TARGET_DIR=.ops/zs1-125-render-tail-work/target`；HOME/CODEX_HOME保持。rustc实际1.94.0，aarch64-apple-darwin，LLVM21.1.8。没有PTY/HTTP调用，也没有修改主库。命令、UTC起止、退出码、源前后hash、原日志大小/hash全部保留在commands。

第一次在2267直接`cargo test --lib render::tests --locked`于编译阶段失败，50项错误来自继承的其它模块不一致（如TUI所需Agent API/模块缺失）；没有运行测试，未修其它文件。随后建立只引用精确`src/render.rs`的最小测试crate，固定ratatui0.30.2、unicode-width0.2.2、unicode-segmentation1.13.3并沿用本地crossterm patch。harness初次以现有lock为种子，`--offline`本地重写它自己的解析范围，后续测试/Clippy全为`--locked --offline`。harness并不等价整个Zenpi激活图或生产二进制。

首次隔离测试19通过、1失败：空code行少了原有styled empty body span，兼容性比较发现并修正。失败原日志保留。之后加入metadata与跨样式Unicode检查。最终原文件hash `b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d` 下，`isolated-tests-03`：23个内联测试全部通过（原11 + 新12），原4个外部集成测试全部通过，doc tests0。不能把多轮重复运行累计成更多独立测试。

新12组覆盖：256KiB一格正文最新尾部与旧head仍保留头部；未裁剪时head/tail的样式/块空行兼容；未闭合code/inline上下文；闭合fence与quote/list/plain区别；tab膨胀及超额输入边界；Unicode/控制/极小极宽viewport；6万宽rule与巨大role共享cells预算；裁剪后不伪造ordered marker；未闭合link扫描界及合法link样式/underscore；metadata与完整小渲染及12万视觉行计数；流式追加索引稳定；跨样式/role Unicode边界。

`isolated-clippy-02 --all-targets --locked --offline -- -D warnings`在同一最终source下退出0。依赖vendor仍打印其既有unused_parens warning；未修改第三方文件，也不声称依赖日志完全无warning。该Clippy范围是精确render模块+其测试和4个集成测试的harness，不是整个产品全目标通过。两次rustfmt和所有中间失败/成功收据保留。

## 交付、接收与限制

after/src/render.rs为60098B/1706行最终候选；product.patch仅一条src/render.rs差异。before/after、精确patch、原命令日志、harness/lock、源输入和测试二进制hash封存。verify.py只在独立临时git init目录检查forward/reverse check/apply、前后逐字节/hash、单文件numstat和非目标sentinel，不执行Cargo或产品。所有原ready保持只读并核验。历史命令路径记录的是实际2267路径；executed-harness是原样上下文，未声称脱离vendor/依赖缓存即可独立运行。离线包完整性验证只依赖包内字节、Python标准库、Git和临时目录。

主控接线前需核对主库src/render.rs仍等于before；不要用候选覆盖新的并发修改。接收仅apply这一产品patch，TUI元数据接线由主控自己的候选处理。回退仅反向此patch，并与主控对新增API的调用联动；不删除用户会话、其它变化或历史证据。真实TUI滚动/resize/复制、release体积/首次启动/RSS、PTY和整项125验收均未在本单完成。

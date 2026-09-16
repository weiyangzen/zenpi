# ZS1-125 主控：单条超长回复尾部与流式阅读锚点

主库已合入单条消息尾部渲染和对应 TUI 接线。真实 TUI 中，同一条普通/围栏回复从 8,180 行继续追加到 9,180 行，读历史位置保持；End 回到底部与后续最终回复均可见。完整 assistant 原文仍只写入一条 journal turn。此次是局部功能修复，不接受 ZS1-125 整项，不证明全 Stage 1 完成。

## 逐文件审查及实际改动

- `src/render.rs`：完整旧文件901行与补丁逐段阅读。旧首部 API 保留；新增尾部 API 和 omitted_visual_lines 元数据，先按同一字素换行算法计数、再保留末尾。Markdown 的围栏/行内样式在完整接受输入内解析，内部列表/代码标记不在被截断续行重新插入。角色缩进计入全列单元预算。tab 延迟展开，CRLF/控制符规范化。跨样式字素采用首字节样式；过宽和行首独立零宽字素以问号代替。修正重复未闭合链接和下划线前缀的重复扫描，未把局部 LinkScan 计数推广为整个解析器复杂度结论。
- `src/tui.rs`：本轮仅 transcript_window 内14行增量；既有尾部多消息窗口、BentoBox、项目分页与目录选择器周边字节保持。使用尾部元数据恢复 `(message ordinal, original visual row)` 身份，合并遗漏提示。当前文件的其余改动均是继承基线，不能归为本轮。
- `tests/tui_transcript_ux.rs`：完整372行旧文件阅读；原测试前缀不变，追加三个针对单条消息尾部、跨8,192行追加锚点以及窄列中文组合字符重排的集成测试。
- `tools/tui_single_message_tail_smoke.py`：新增独立真实终端回归脚本，普通/代码围栏各一个TUI/HTTP请求，provider门控追加与完成。复用已读的Terminal/screen_text及drain/send，只使用临时普通文件与loopback fixture，HOME/CODEX_HOME环境原值保留。新evidence路径必须不存在，不覆盖原始证据。

B renderer 与 wiring 两个ready完整冻结、逐个离线verify，精确patch正反回放及无关哨兵通过。主库逐文件比对before、check/apply后逐字节比对after。在197个现有Rust/vendor/Cargo输入中仅上述3个Rust文件改变，194个其它输入保持。输入索引不包含新Python脚本，脚本及3个实际导入的工具另有probe-inputs.json与executed-tools字节快照。无reset/stash/clean或覆盖继承变更。

## 主库运行证据

原生 `cargo +stable-aarch64-apple-darwin`，全部依赖命令 `--offline --locked`：

- render内联23、tui::tests内联16、render_markdown集成4、tui_transcript_ux集成16，共59项通过。这里16个TUI内联仅为实际过滤范围，不能写成所有TUI测试或全库测试。
- Clippy `--lib --tests -- -D warnings`、`fmt --all -- --check`、debug zenpi构建均退出0。保留既有vendor编译warning。未在本轮重跑完整lib或新release预算。
- 本轮冻结debug二进制 SHA256 `bdf901227bb2b20ae422f0802c3b361381952ba861b040320c401e6625f2cfa0`。
- 同一脚本对旧debug `d150d6ef355e15bbb32b5176a15e984f2a252d396a2c7c1625cd029dab50bb52` 实际运行一次：两场景合计10/14通过，4项失败是End之后尾部与最终回复不可见；完整单条journal、HTTP次数及终端恢复仍通过。旧程序未改写重建，旧输出全部保留。
- 新debug同一脚本一次14/14通过，两TUI、两真实loopback HTTP；准确比较第一次上翻的屏幕行在追加后不变，End显示ROW_09179，最终SINGLE_STREAM_FINAL可见。逐一比较journal assistant内容与完整期望字符串一致且数量为1。
- 既有真实入口：多消息18,000行及新回复5/5通过（1headless、1TUI、4HTTP）；交互11/11通过（2TUI、3HTTP），包含Reasoning折叠、答案/代码复制、鼠标Latest、工具输出检查、inspector中断、窄屏项目导航及终端恢复。原脚本/辅助工具冻结。
- 因源码实际改变而重跑这些回归；没有改变门限、预热或失败后重复刷绿。

C 独立复核的输入是同一render after字节。主控已读取其完整review、8个独立测试与verify，核对140载荷并运行只读verify成功。C 8个新增语义测试与复跑23+4属于隔离单模块harness，**不计入上述主库59项**，也不借作实际TUI证明。包保留完整harness/pins/lock与原始命令收据。C未提供产品修复，因为没有复现缺陷。

## 精确边界及后续

接受输入仍至多256KiB，超限保留既有首部字节策略；本轮尾部保证限定在合法接受字节内，超过该cap的最新内容未被证明可见。单消息再长的完整文档浏览/复制配额等仍需整项审查。宽度变化及Markdown追加改变早前块结构时视觉行号可能变化，不宣称语义/字节锚点永远不变。主库正常终端宽度预算保留8,192行和4Mi单元，缺省正文至少1列；极端只有一个保留行时遗漏提示仍可能占该行。

本轮没有合入A异步session browser候选，也没有合入C /diff身份候选。最后release 1d7b2e61…早于当前源码，不能用它覆盖新尾部/附件/headless修复；新release和固定预算仍待验证。没有Linux、Windows、远程Actions或真实外部provider证明。保持ZS1-125 `[ ]`，主控局部验证状态单独展示。

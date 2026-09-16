## 主控独立逐文件验收 — ZS1-094 / 3.1.21

主控已按顺序完整读取当前 src/layout.rs L1–320、321–640、641–960、961–1260、1261–1567至EOF，工具记录dc6953/c35db6/5edf73/29249d/8ac48d。54632 B、SHA256 218c1b1cb7853da8f5c03b51e09d552f0634efb82284e029cbcfaeb434dac05a，与冻结登记逐字节一致；不改写基线。完整候选报告34842 B / SHA256 4a12b4cf394ee52cdc00631dbc29bdade4f6e6cca484301e60476176f7cbda9a已顺序读至EOF（00ddaa/188a62），新的verify_packet.py也完整静态读完。77个函数的签名、实现、类型、注释、全部条件/错误分支纳入语义审阅，数量仅是导航，不替代全文。

文件是无IO、无async、无取消和终端控制的纯布局/偏好模型。五个TabId是旧工作区preset，不是项目路径或session身份；19个PaneId仅Browser/Terminal为optional。PaneRect半开、饱和边界，empty不相交；capability是宿主声明。持久化只保留比例、行权、折叠、焦点，不持久化tab或capability，宿主恢复后需施加实际能力。五preset与各列/行minimum、权重逐项核对；比例按扣除minimum后的剩余空间分配，并非整个矩形严格百分比。

bounded先归一化到100再限5..90并修余量；delta0直接返回未归一化值。列调整donor次序按列固定，分配取整余数相等时后index优先。空分配返回空Vec；算法注释“总和等于total”限非空对象。u32乘法只按实际u16尺寸和内置有界preset评估，不推广为任意大输入。可用性、断点、用户折叠、optional宽度折叠、按列/行分配的先后顺序已核对，computed折叠不回写用户折叠。Zero隐藏可用pane；Narrow显示有效焦点或首required，但循环候选包含其余可用未折叠pane；Compact折右列，Standard/Wide同分支。

尤其保留低高度边界：Visible枚举不保证矩形非空，非Narrow focusable当前也可能包含零高矩形；render另行跳过空矩形。无候选focus_cycle返回None但不清旧focused。方向导航先横纵几何重叠及主/次轴距离，找不到则按方向循环，不能写成永远只跳几何邻居。adjust_focused_split可先改变focused、最后因比例到界返回false。低层setters不做成员/capability合法性检查，reset保留tab/capability。上述是静态合同与待测边界，不是已复现产品失败。

Preferences验证schema1、profile ASCII命名1..128、最多64profile、tab集合、比例5..90总100、权重1..1000、pane归属。64折叠上限在19个typed PaneId集合下不构成可达边界；validate不要求焦点展开或capability可用。外层偏好结构deny_unknown_fields，ColumnRatios和legacy LayoutModel没有该限制。from_json在Value解析前限制256KiB；needs_migration看schema键是否缺失，migrate看as_u64，非整数/字符串/null/负数等带合法legacy字段时存在回退且标记不同步的静态输入族，尚未运行验证。过大正整数转u16::MAX后报SchemaVersion。

set_model验证现值及新值、clone更新并验证next后才替换，profile65失败不提前提交；它不证明最终pretty输出小于256KiB。to_json在分配编码Vec后检查大小。model_for全validate，tab_state/reset只验请求key；预存空profile的reset_tab可移除profile却返回false。模型不提供磁盘原子性或崩溃恢复，不能拿内存验证代替config/session宿主验收。

主控完整读tests/layout.rs 277行11个测试和tests/layout_persistence.rs 166行7个测试（ca3329），本轮没有产品执行。现有测试涉及preset、断点、min/nonoverlap、optional、窄屏、折叠、循环方向、split/reset、配置往返与legacy等；没有覆盖每个异常schema、零高导航或每个极限分配。待测判据按候选矩阵保留，测试名字和18声明不等于本轮18通过。200×40 full-capability手算列56/85/59和左行17/12/11只用于确认分配语义，不计运行。

本轮主控新读当前TUI 7c0d9e5c2cb5f351412aa6e98ac233f5c55580ee1885a431653d8c91b908ba4b的L3438–3506、3566–3785、7790–7878，完整render_bentobox及render_workspace L7292–7458。恢复、focus/collapse/preset、dirty、capability、adapter坐标转换分别有更强宿主约束。workspace_viewport目前采用last_area完整frame；实际workspace绘制扣除顶栏、header、动态input、footer后使用workspace_area。差异静态确认，是否形成用户可达不可见焦点尚待实际反例，另归TUI产品修复；不在094接受中宣告已解决。模型可复用不等于headless存在某协议入口。没有把本次有界上下文说成完整TUI/keyboard/mouse审阅。

候选捕获645936 TUI时F2进行中的描述保留为历史。当前F2会话列表与F1历史Super已分别合入，主控公开记录为ZS1-126/master-session-selection-3.1.21/review.md和ZS1-122/master-history-super-3.1.21/review.md。那些143/128 Rust及18 PTY是各自整合的历史结果，不重复运行、不累加为094测试；worker失败/旧包也保持原样。BentoBox及顶部加号直接选择工作目录的交互没有更改。

完整静态审阅后，将portable verifier复制到新的root-offline目录，仅运行一次：267项结构检查通过、0失败、0产品执行。检查覆盖manifest/全部payload、连续阅读绑定、上下文/报告身份、正反补丁精确性与旧包留存；它不自动证明语义，验收依据是上面的独立源码审阅。只接受094 G-FILE完整理解，不接受090目录、120项目身份、128完整交互、085整文件或全阶段。回退仅撤销此报告/receipt/清单状态，不覆盖源码和用户数据。

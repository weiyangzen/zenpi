# ZS1-308 · key_hint.rs 完整单文件复核（3.1.21）

本文件 understand 候选，主控 G-FILE 待审；只交付 `Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/key_hint.rs_learn.md`。不接受目录、整TUI、122/125/131或任何平台实现，不改产品、主库权威文件、claims、蓝图、selector、receipt或BentoBox。run `zenpi-stage1-20260911`，requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；捕获blueprint snapshot `ca9704da623173c944c5040aa6eb90a731219a8e029a89cd21b4f2f97e99e078`。所有结论限定捕获身份和实读范围。

正式源 `/Users/wangweiyang/GitHub/codex/codex-rs/tui/src/key_hint.rs`，HEAD `b3b3d262787f4902a7449f17d793241a34d311ad`，**3306 B / 112行 / SHA256 `07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8`**。本轮独立完整顺读1–112、连续 `[0,3306)`，包含全部平台分支、注解和样式；与冻结3.1.21 manifest及旧308源逐字节相同、delta0。14个文本函数定义保留两个from、两个is_altgr；平台cfg选中其中13个，未编译验证。`#[cfg(test)]`只定义ALT常量，正式测试定义0、执行0。

[正式源](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/capture/codex/codex-rs/tui/src/key_hint.rs)；[完整读账本](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/read-binding.json)；[逐单元字节与语义](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/source-units.json)；[14函数（不按名字去重）](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/function-inventory.json)。

## 旧链与当前范围

旧worker2267 `zs1-308-ready`65 regular与聚合17 regular全部原样保存。旧完整报告 22332 B /183行 / `2f564627276bed05bdd345d5e5a592e354bfe777cf4adf928d96c406ef7b78c9`，本轮按1–64、65–126、127–183全文读；1537 B原报告 `37e21c9bd266b6acc24f596b467e291e1c7abaa42b025414ee1ef56a5da36755`亦全文读。旧manifest `305bf26e6b67fd850851bb27a836a444aecaa9eb3bef2397eba523097052eb7c`，旧read-log `6c8fa8df8210b469dc79dfe36e055dd63e6811d63ddf08db438d2e32573a93a7`含112行连续全字节链。原报告“AltGr保证文字不吞”已在旧完整报告中收窄，本轮进一步读到textarea中CtrlAlt+h先于AltGr的实际例外；不会恢复成全局保证。

旧TUI579524 B/d2a98934…，当前捕获 **638305 B /15607行 / `0deb8e0e13cfdad0aa00e07d79caf573e9aa36095a9bcefb4219ed8f70936557`**。大文件只按ledger有界阅读；整份复制用于hash绑定，不产生完整学习信用。[历史包](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/history/zs1-308-ready/manifest.json)、[精确旧/新片段比较](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/history-comparison.json)保存旧context全文件hash及每段是否在当前字节精确找到。本轮补读现有终端进入/退出和生产事件接线、当前目标5个测试、源textarea的AltGr例外、完整source shortcut descriptor片段；不扩大其它owner工作。

## 全部源单元

|行|单元|语义、分支与边界|
|---|---|---|
|1–17|平台常量与依赖|ALT_PREFIX在cfg(test)始终为⌥ + ；非test macOS相同，非test非macOS为alt + 。CTRL/SHIFT恒为ctrl + /shift + 。cfg(test)不是测试定义，不检测运行时终端。|
|18–28|KeyBinding与new|私有key/modifiers，Clone/Copy/Debug/Eq/PartialEq；const new原样保存，不归一化键码、大小写或位集，不注册动作。|
|29–35|is_press|要求完整KeyCode与所有modifier位精确相等，并允许Press或Repeat、拒Release。不检查event.state等字段；不处理焦点或上游repeat过滤。|
|36–39|plain|const new(key,NONE)，不添加默认modifier。|
|40–43|alt|const new(key,ALT)，与实际OS是否产出Alt事件无关。|
|44–47|shift|const new(key,SHIFT)，不把Shift+Tab转换成BackTab。|
|48–51|ctrl|const new(key,CONTROL)，不是contains匹配。|
|52–55|ctrl_alt|const new(key,CONTROL.union(ALT))，不做AltGr例外；分类helper是另一条路径。|
|56–69|modifiers_to_string|contains依次追加CONTROL→SHIFT→ALT前缀；Super/Hyper/Meta等位不显示但仍保留在binding中；不同binding可有同文字，不可把提示作身份。|
|70–74|owned From|借用自身并委托borrowed conversion，不另写一套格式规则。|
|75–93|borrowed From|八专名：Enter/Space/四箭头/PageUp/PageDown；其余用KeyCode Display再ASCII小写。生成拥有String的Span static，无借用悬挂；字符A可显示a但matcher仍匹配A。|
|94–97|key_hint_style|Style default.dim()；未指定色彩，没有裁剪、换行、width预算、国际化、字形fallback或可见性过滤。|
|98–101|has_ctrl_or_alt|CONTROL或ALT存在且不是is_altgr；SHIFT-only/SUPER-only为false，不等于完全无修饰键。|
|102–107|Windows is_altgr|contains ALT与CONTROL即true，额外SHIFT等也不影响；不能区分真实AltGr和人工CtrlAlt，未识别左右Alt、布局或IME。|
|108–112|非Windows is_altgr|恒false，WSL的Linux编译同样如此。与footer运行时is_wsl分支不同，不提供跨平台字符输入保证。|

本文件不做I/O、持久化、异步任务、计时、焦点或权限；没有Result、注册表、重绑定配置、提示本地化或字体能力探测。字符串分配和Span构造不是资源门禁通过证据。匹配完整位集但显示仅三个前缀，故同一显示文字可以对应不同事件；Char('A')显示a也不使它匹配Char('a')。formatter不是输入的反向解析器，更不能作为审批身份。

|配置/输入|静态结果|未验证边界|
|---|---|---|
|cfg(test)，任意目标|Alt前缀⌥ + |测试快照不能证明Linux/Windows生产文案|
|non-test macOS / 非macOS|⌥ + / alt + |编译目标，不探测实际键盘/终端|
|NONE、SHIFT-only、SUPER-only|has_ctrl_or_alt=false|不是“无任何修饰键”|
|CONTROL-only、ALT-only|has_ctrl_or_alt=true|两OS分支一致|
|Windows CONTROL+ALT，可再带SHIFT|is_altgr=true，has_ctrl_or_alt=false|人工CtrlAlt同样被归类，不识别实际AltGr来源|
|非Windows CONTROL+ALT（含Linux/WSL）|is_altgr=false，has_ctrl_or_alt=true|WSL运行时提示选择不改变此cfg|
|code/mods完全相同，Press或Repeat|is_press=true|caller可能只派Press或有别的匹配规则|
|Release、额外modifier位、Char大小写不同|不匹配原binding|其它状态flush可能发生在上游，不能说应用全无副作用|

## 调用方：可见性、AltGr与真实动作

footer883–1057以ShortcutBinding.matches调用DisplayCondition，检查Always、ShiftEnter正/反条件、WSL、协作模式；binding_for取第一个满足项，overlay_entry转Span并追加文案。Newline根据设置用ShiftEnter或CtrlJ；PasteImage WSL CtrlAltV置于Always CtrlV之前，顺序影响实际可见提示；ChangeMode ShiftTab仅协作模式启用时出现。这只是提示选择，不验证剪贴板或模式切换动作。

源composer3155–3177问号overlay只允许Press、has_ctrl_or_alt=false、空composer且非paste；3007–3045与3093–3113用同helper分类/清burst。新读textarea330–344明确精确CtrlAlt+h先删除上个词，之后才is_altgr(modifiers)插入Char。因此Windows AltGr启发式并不能保证每个CtrlAlt字符都当普通字；这些实际优先顺序比helper孤立真值重要。其余textarea输入方法没有全文接受。

源approval372–418也混用三种规则：CtrlA full preview仅Press但contains CONTROL；o选择来源thread仅Press，不限制modifiers；其余选项才调用KeyBinding.is_press并允许Repeat。formatter严格匹配不能推广到整个overlay。BottomPane双击退出开关为false，show_quit_shortcut立即return；保留1秒常量和后续异步/线程定时分支不说明实验启用。308不创建取消/退出状态机。

## 与当前zenpi的逐能力映射

|能力/源范围|当前上下文与行为|差异或范围限制|最小可运行判据（全部未执行）|
|---|---|---|---|
|C01 binding身份/事件种类 / 18–55|TUI5532–5596、5645–5655：handle_key拒Release但直接API可收Repeat；实际handle_event_at后段仅Press，前段普通字可处理Repeat。整体先flush，再拒Release。|不把源is_press允许Repeat直接映射为主host动作可重复，也不把Release无本键动作写成无任何flush副作用。|same code+mods的Press/Repeat/Release、额外Shift/Super和Char大小写；分别走直接与实际事件入口，记录草稿、动作和待flush文本。|
|C02 平台显示与AltGr / 9–16、98–112|textarea330–344；TUI5545–5567、5817–5992：source AltGr先被helper识别，但textarea先处理精确CtrlAlt+h删除词，再到AltGr字符；目标ordinary拒Ctrl/Alt/Super，Ctrl分支contains且没有已读AltGr例外。|源只是启发式，不能保证所有AltGr字符安全；目标真实Windows输入未测，不把缺helper直接称已复现丢字。|Windows真实AltGr与人工CtrlAlt、CtrlAltShift和CtrlAlt+h；Linux/WSL对照，记录原始事件和最后字符/命令，避免误触删除或退出。|
|C03 显示碰撞与键名 / 56–93|目标footer7021–7055与键路由5817–6019：源前缀顺序固定且忽略Super等；八个特殊键名，fallback仅ASCII小写。目标手写Ctrl/Alt文本，无共用描述符。|文字不能当授权或匹配身份；键名可读性与crossterm Display依赖、Unicode字形另验，不假造fallback输出。|owned/borrowed Span一致；NONE与SUPER同字符文字碰撞但完整mods不同；ASCII大写/非ASCII、Tab/BackTab/Page/方向逐项显示与匹配。|
|C04 提示条件与动作owner / 全文件无条件过滤|footer883–1057；TUI5743–5748、7021–7055：source descriptor first matching按WSL/ShiftEnter/collaboration决定提示；target宽屏常驻Ctrl-G editor，但有slash choices时路由返回None。|源formatter只生成Span；显示入口不等于动作在当前状态可用。目标可以保留固定帮助，但需说明忙碌/modal/菜单条件。|WSL开关、ShiftEnter设置、协作模式、菜单出现/消失；校验可见文本对应当前动作，不能用无效提示冒充可发现性。|
|C05 样式/布局/优先级 / 94–96|TUI1007–1037、7021–7055、1140–1215：源DIM无布局；目标warning > background approvals > paste > width99/100 general，裁width；后台无unsaved为Yellow，其余DarkGray，零面积return。|DarkGray不是DIM；保存错误可遮审批提示，后台/粘贴可遮通用快捷键，tiny modal可能不显示。保留BentoBox，不换布局。|0/1列、99/100列、Unicode项目名、同时warning/approval/paste；检查裁剪后关键操作可发现，未测对比度/真实终端字形。|
|C06 审批焦点与选择 / 29–33|Codex approval372–418；TUI1052–1131：source CtrlA contains、o不限制mods，余下选项才KeyBinding.is_press；target精确AltA打开当前项目，focused CtrlC精确，其余非空mods拒，y/n只选择Enter确认。|严格binding不是全overlay保证；后台提示要求先切tab有实际意义。审批授权仍coordinator，报告不以keypress代替权限确认。|当前/后台项目、picker/审批焦点、CtrlAltA、Repeat y/Enter、Esc保稿；核验来源tuple与只一次明确决策，源/目标分支分别观测。|
|C07 取消、退出与恢复 / 全文件无计时/任务|bottompane112–128、656–678；TUI5823–5829、6113–6128：源双击退出开关false，1s常量及timer代码仍在；目标空且busy CtrlC中断，空CtrlC/D退出，非空CtrlC中断，Esc空中断非空保稿或恢复history。|不移植已禁用双击退出实验；helper本身无取消协议/持久状态。模式前置拦截仍影响CtrlC。|idle/busy、空/非空、history/picker/browser/approval；确认正确owner、中断后可继续、退出恢复；不跑本轮终端实测。|
|C08 BentoBox导航/换行 / 44–54、79–89|TUI5871–5975：目标有文本时CtrlLeft/Right移动词，空文本Ctrl方向焦点/水平CtrlShift调整split；Tab/ShiftTab/BackTab空稿焦点，CtrlTab空稿项目；CtrlJ/CtrlEnter换行。|源ShiftTab是binding表达，不是BackTab归一化；目标接受两种反向focus键码。相同CtrlT在source transcript、目标directory picker，有意语义不同。|两种ShiftTab编码、空/非空、CtrlShift方向、CtrlJ/Enter；真实terminal能否区分由输入协议决定，不以构造KeyEvent代替host。|
|C09 文字与paste分类 / 98–112|composer3007–3045、3093–3113、3155–3177；TUI5532–5596、5988–5992：源has_ctrl_or_alt决定burst且问号overlay必须Press/空稿/非paste；目标ordinary排Super但最终Char只排Alt/Control，Super+Char Press可沿代码落普通插入。|这是确定的分支不一致与静态可达候选，真实终端发不发Super未测；不要声称已造成用户数据损坏。|Super+Char、CTRLALT、Repeat普通字、Release前持字、Unicode IME；实际事件队列入口确认不误提交、不吞已有字。|
|C10 copy/reasoning/blocks / 全文件不执行动作|TUI5713–5733：精确AltR切reasoning，AltB开transcript browser，AltC copy_latest_answer；无答复显示No assistant answer to copy。|已有真实调用点，不能沿用原报告建议“新增”当现缺口；剪贴板OS成功/失败与块选择范围未审。|额外Shift/Super不得悄悄当AltC；空答复、有效答复、剪贴板拒绝反馈与焦点恢复另验。|
|C11 终端编码与清理 / 全文件不协商协议|TUI12361–12402、13503–13545、13614–13641：生产poll/read→handle_event；enter raw/alternate/bracketed paste/mouse/hide，错误尝试回退；leave尝试恢复并忽略错误，Drop调用leave。|已读enter无键盘增强协商；不能保证所有终端区分CtrlJ/Enter/Repeat/Release。terminal signals/editor底层合同未扩读，不认131完成。|后续记录OS、终端、键盘布局、原始事件、实际恢复；当前零平台/终端实验，保持既有预算。|
|C12 测试/历史范围 / 9–14并非测试|footer1706–1742；tests/tui_composer31–47、133–144、379–445：源测试0；footer1测试依运行时WSL取期望，只跑所处一分支；目标5测试只读，涵盖编辑、Release直接API、ASCII hold、burst与Unicode。|6项context tests均未运行，未含全AltGr/Super平台矩阵；历史release和budget保存原件不计当前通过。|未来有限单元与host测试分别记录，不把cfg(test)的⌥快照当非macOS生产证据或旧7套release代替本项平台验证。|

当前host以poll/read→state.handle_event→handle_event_at进入。handle_event_at先flush_ordinary_paste，随后Release返回；composing要求picker/browser/history search/当前focused approval均不占用。普通字分支排除ALT/CONTROL/SUPER，却接受Repeat，ASCII进入缓冲，孤立非ASCII立即插入并重开菜单。后段只把Press交handle_key；后者仅拒Release，直接API仍可能处理Repeat。已有直接handle_key的Release测试不覆盖前置flush，不可写成整个应用Release绝无状态变化。

普通Ctrl快捷键用contains CONTROL，精确ALT分支则承载reasoning/copy/blocks；审批focus使用精确modifier且拒其它非空位。目标最后Char分支只排Alt/Control，不排Super：Super+Char Press跳过ordinary分支后可以沿已读代码到insert_text。这里记录静态可达条件，未接真实Super键事件；同样没有实测Windows CtrlAlt/AltGr。后续应按动作和文本输入分别定modifier策略，不能机械把所有contains改精确相等，破坏既有CtrlShift焦点或编辑组合。

目标footer手写两种宽度提示：<100为Enter/CtrlJ/CtrlR/CtrlC及/help input，宽屏加CtrlG/CtrlU/CtrlY和AltR/C/B；保存错误优先覆盖，后台审批其次，再是相邻paste提示。background最多3个非当前项目名，每名28宽，其余+N projects，再总体裁宽；AltA仅打开当前项目的审批，所以switch tab是必要步骤。焦点审批tiny宽<4或高<5不画modal；其他状态显示y/n选择、Enter确认、Esc draft、CtrlC cancel，不能将“存在footer字符串”当任何窗口下可发现性通过。

DIM是源Span属性，目标DarkGray/Yellow是颜色选择；不声称视觉对比度等价。CtrlG宽屏文案可能出现而slash choices存在时路由返回None，是提示条件未表达的具体差距。CtrlT的目标动作是directory picker，源descriptor是transcript，不应照抄文字。空稿Tab/ShiftTab/BackTab是BentoBox焦点，CtrlTab是项目；有稿CtrlLeft/Right是词移动，空稿改为pane焦点，水平CtrlShift方向调整split。当前行为需保留，用平台协商/真实事件证明支持而非仅KeyEvent构造。

TerminalGuard enter请求raw、alternate、bracketed paste、mouse、hide，execute失败尝试反向恢复并返回错误；已读路径没有键盘增强协商。leave在active时尝试显示光标/禁mouse与paste/退alternate/关raw，Unix再尝试reclaim/restore并释放signals；多个错误被忽略，Drop调用leave。这里只证明清理调用点，不证明实际TTY恢复或外部编辑器/信号全合同；没有任何本轮终端/131/FIFO实验，也没有运行新预算或更改阈值。

## 测试、历史证据与待验范围

正式源测试0。源footer完整读paste_image_shortcut_prefers_ctrl_alt_v_under_wsl（1706–1741），只用当前Linux WSL检测或非Linux false决定actual/expected；不是强制true/false双分支参数化测试，更不是实际paste。

目标5个测试函数全文只读：multiline_word_and_line_commands_keep_wide_columns（31–47）；escape_keeps_draft_and_key_release_never_types_or_submits（133–144）；ordinary_ascii_hold_flush_and_deliberate_enter_have_bounded_latency（379–392）；ordinary_multiline_burst_and_trailing_enter_never_submit_before_quiet_window（394–422）；ordinary_unicode_ime_is_immediate_and_modified_keys_flush_without_losing_text（424–445）。有精确文本/动作断言，ASCII夹具按windows选30ms、其它8ms是原测试输入，不是本轮性能结果。总6项context测试执行0；未证明AltGr/Super/按键协议平台矩阵或所有提示宽度。没有把测试字符串里的命令执行，也没有跑任何旧smoke。

旧65文件包包含旧release reviews/manifest、7套UX结果与budget原件，全部字节保留，但本轮只重读旧308报告和身份链，不重新语义审核所有release raw日志、不重新跑仪器或产品、不重新计历史8门禁/冷启动/7套交互。旧release complete=false、以前冷启动失败和packaging-attempts继续保留；不把旧预算通过提升成当前输入路径或平台通过。

准备时唯一新失败是按旧定位请求footer1706–1743，当前文件仅1742行，read.py在写ledger前拒绝；随后实测行数并完整读1706–1742。旧报告的超出尾行标记未悄悄沿用到新账本。AGENTS此前已全文读且当前hash相同，本轮无Rust修改不触发fmt/测试操作。所有原ready和tracked文件在冻结时核对保全。

## 精确读取与交付

仅key_hint.rs正式完整112行；其余均context-only。read-binding记录每段源全文件hash、行范围、连续byte和raw片段，少量边界辅助行不接受相邻完整函数或文件。未读内容明确排除，包括crossterm Display/平台解码、完整clipboard、布局与editor信号实现。下表仅新capture读取范围；旧完整报告3段另在账本。

|文件|捕获SHA256|实际阅读行|
|---|---|---|
|capture/codex/codex-rs/tui/src/key_hint.rs|`07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8`|1–112|
|capture/codex/codex-rs/tui/src/bottom_pane/footer.rs|`f932785cb38c86e897dd709daea9c3214806d3c1845b984cd9d5295520144a64`|880–1062, 1706–1742, 843–879|
|capture/codex/codex-rs/tui/src/bottom_pane/chat_composer.rs|`234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da`|3007–3045, 3090–3174, 3175–3185|
|capture/codex/codex-rs/tui/src/bottom_pane/textarea.rs|`8cb5241c9e818210bfff63975e703f45a25a3da91bdb395667893903f7cb4b45`|330–366|
|capture/codex/codex-rs/tui/src/bottom_pane/mod.rs|`97af02eed39dbfdf69d1ec3e14dc13b9a91f4b33f5e4b985cc1f2cbae4105823`|112–128, 656–680|
|capture/codex/codex-rs/tui/src/bottom_pane/approval_overlay.rs|`c924923b402b198f8f77b249f0d37892896c0b84002c0f6e48330917da80cecc`|370–422|
|capture/zenpi/src/tui.rs|`0deb8e0e13cfdad0aa00e07d79caf573e9aa36095a9bcefb4219ed8f70936557`|5532–5608, 5640–5749, 5816–6019, 6090–6134, 7012–7058, 998–1100, 1140–1208, 1101–1139, 1209–1228, 13503–13545, 13638–13662, 12361–12402, 13607–13637|
|capture/zenpi/tests/tui_composer.rs|`c56a654e5f03ef691f9bb4ffb0ae57ee3577b6630309dee9367debe503da210b`|133–146, 379–446, 31–48|
|capture/zenpi/Docs/stage_1_v3_pi_mono_blueprint.md|`ca9704da623173c944c5040aa6eb90a731219a8e029a89cd21b4f2f97e99e078`|1–13, 135–143, 424–429|

唯一[新增报告补丁](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/candidate.patch)和[反向补丁](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/rollback.patch)只作用本标准报告；冻结前只读核验主库canonical不存在。[便携验证器](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/verify.py)核对冻结集合、源与旧链、112行连续覆盖、14定义/15单元/12映射、所有实读片段及selector/source manifest；只在独立临时git做check/apply/重复拒绝/反向回滚并验证无关sentinel保留。这是离线完整性与交付校验，不代替master G-FILE语义审阅或产品G-HOST。冻结后最多一次验证，日志与运行记录存ready外新私有目录，不回写冻结manifest。

[捕获清单](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/capture.json)；[旧链与片段比较](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/history-comparison.json)；[准备问题记录](/Users/wangweiyang/.codex/worktrees/ff51/zenpi/.ops/reference308-keys321-ready/evidence/preparation-issues.json)。当前产品/Cargo/PTY/HTTP/平台/性能运行0；未创建task/subagent、未改HOME/CODEX_HOME。交付后停在308边界，主控另行决定接收。

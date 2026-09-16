## ZS1-308 主控独立单文件验收 — 3.1.21

接受且仅接受 `codex-rs/tui/src/key_hint.rs` 的完整源理解与当前 Zenpi 映射。本次完整阅读 3306 字节、112 行、连续 [0,3306)，SHA256 `07f34aab630b2658560d2d4e73719481ec8638e33eb38401d7a23643689f13c8`；完整阅读 22556 字节候选和只读 verify.py。源文件没有测试函数；14 个文本函数定义包含两个 from 与两个 cfg is_altgr，单一 cfg 选择 13 个，不虚报编译或跨平台运行。

完整源语义确认：KeyBinding 保存原始 KeyCode 与完整 modifiers，is_press 同时要求精确键码、精确位集、Press/Repeat，拒绝 Release，但不检查 event.state。五个 const 便捷构造器仅设置固定组合，不归一化大小写、BackTab 或 Shift。formatter 只按 Control→Shift→Alt 显示前缀，其它 modifier 位仍参与 matcher 却不显示；fallback 是 KeyCode Display 后 ASCII 小写，不能据此推断未读的 Display 实现或 Unicode 大小写变化。owned From 委托 borrowed From，生成拥有 String 的 static Span；dim 样式没有宽度裁剪、字体探测、焦点选择或动作注册。

Alt 前缀在 cfg(test) 一律为⌥，生产由编译目标 macOS 与否决定。has_ctrl_or_alt 的否定不是“没有修饰键”：Shift/Super 可存在；Windows is_altgr 仅以同时 contains Control/Alt 判断，额外 Shift 不改变结果，也不识别真实 AltGr 来源。非 Windows 分支恒 false，运行时 WSL 提示选择不改变此 cfg。helper 不执行 IO、权限、异步任务、取消、退出或持久化。

主控另完整阅读候选记录的所有源码 context 片段：footer 843–879、880–1062、1706–1742；composer 3007–3045、3090–3174、3175–3185；textarea 330–366；approval_overlay 370–422；bottom_pane/mod 112–128、656–680。它们是有界 context，不接受对应大文件。确认 footer 按第一个满足条件的 binding 决定提示，WSL 图片快捷键候选优先于 Always；测试按所处运行环境选择单支期望，不证明两平台分支均执行。textarea 的精确 CtrlAlt+h 删除词先于 AltGr 插字；approval 的 CtrlA 是 contains/Press，o 只限 Press，剩余选择才走 KeyBinding，故不能把严格 matcher 的保证推广到全 UI。双击退出实验开关 false，保留 timer 代码不代表已启用。

目标方面，新读完候选全部 TUI context 与五个完整测试片段，范围如随包 ledger；其中 5532–5608、5640–5749、5816–6019、6090–6134 是捕获时的事件/编辑路径，998–1228 分块为审批与提示，7012–7058 为 footer，12361–12402、13503–13545、13607–13662 分块为事件循环和终端清理。测试读取范围为31–48、133–146、379–446。复制的完整 TUI、完整测试文件、旧历史完整报告或所有 release 日志不据此获得全量阅读信用。

捕获之后主控已独立修复 C04/C09 中两处目标问题。当前 TUI `b15661a160a855ef974e82dfb40a63c8d0c5777623cd553d1d4cf2069d8b59a1` 与测试 `30d9b19bc250409bac1e4254b7237f11cf8de625913d55b669a044665e89d7c1`，相对候选捕获恰为此前主控独立审阅并合入的两文件补丁：最终 Char 分支也排除 Super；Ctrl-G action/footer 共享模态和 slash choices 可用性判断。旧候选的“当前仍有该缺口”仅保留为当时观察，不是本次最终状态。完整差异保存在当前审计附件，且与已集成 candidate 精确相同。

此前主控实际三失败→157项回归通过及两次新原生 ARM debug PTY 的证据见 `Docs/quality/stage1/ZS1-122/master-key-dispatch-3.1.21/review.md`。此处只复用该已审核局部修复链，不新执行或重新计数。Super/Repeat/Release 仍仅为生产入口合成事件证明，真实终端对照验证的是菜单提示、Ctrl-G 拦截、Esc、Unicode 和 Ctrl-U/Y/D；没有 Windows AltGr、键盘增强协议或完整外部编辑器/终端恢复证明。

其余当前映射保持明确边界：handle_event_at 可在 Release 早退前 flush，直接 handle_key 与主事件入口的 Repeat 处理不同；精确 Alt、contains Control 与审批精确规则各有既有用途，不机械统一。Ctrl-T 是 Zenpi 目录选择，Tab/BackTab 为空稿 BentoBox 焦点，CtrlTab 为项目，不能照抄源 transcript/queue 文案。warning、后台审批、相邻粘贴与宽度优先级会隐藏通用提示；DarkGray/Yellow 不等于源 dim。TerminalGuard 发起清理且忽略部分错误不证明所有 TTY 均已还原。

新复制冻结包 manifest `0eb674eb2fff8b6a2f25ba21ce439d9c06b46d526acf25729e5064938e85c77f`，148 payload。主控从 /tmp 新执行一次已读离线 verifier，176 项通过；它在新 TemporaryDirectory 做实际报告补丁 check/apply/重复拒绝/rollback 并保留 sentinel，没有运行源代码、产品或旧 probe。所有捕获 source/context 当前身份相同；只有明确的两目标文件和推进后的 authority 变化，已逐项记录。历史包、失败和旧预算记录原样保存，不以哈希验证冒称重新审阅所有历史运行。

本项 G-FILE 依据独立完整源理解、调用方限定及目标差异接受，当前 G-STAGE 逐项检查另存实际命令记录。无新源测试/产品测试或平台实验，不接受 308 父目录、其它参考文件、122/125/130/131、全 TUI 或 Stage1。撤回范围仅本项报告/receipt/状态，保留其它所有独立记录和产品修复。

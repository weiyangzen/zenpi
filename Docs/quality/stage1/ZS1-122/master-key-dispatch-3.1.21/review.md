# TUI 输入分派与 Ctrl-G 提示：主控集成复核

两处局部修复已进入主库，涉及 `src/tui.rs` 和 `tests/tui_composer.rs`。没有接受整个 ZS1-122、ZS1-130、目标 TUI 文件或 Stage1；BentoBox 布局代码和顶部加号目录选择逻辑未改。

## 实际改动与独立审阅

主控完整阅读 worker 报告、补丁、5 个新测试、只读校验器及最终 PTY 脚本，另外阅读当前 handle_event_at/handle_key 的 ordinary-paste、Release/Repeat、模态焦点与 Ctrl-G 分派，approval_key 和 footer 生产上下文。确认普通输入分类已排除 Super，但未分类事件进入 handle_key 后最终 Char 分支原本只排除 Control/Alt，故 Super+Char 在 Press 下仍插入草稿。修复对齐同一分支的 Super 排除，不改变终端增强协议，也不统一改写其它已存在的组合键匹配规则。

新增 external_editor_shortcut_available 同时用于 Ctrl-G action 和宽 footer。它要求无 directory picker、transcript browser、history search、聚焦且非空的审批，并无 slash choices。原入口对多数 modal 已优先返回；统一判断使提示反映同一可用性。busy 草稿仍可用，Ctrl-T 目录选择保持。该函数未承诺所有 footer 快捷键、所有外部编辑器配置或整个模态系统均已审计。

worker 冻结包 manifest `c1ecdc7f97eb10be0448744fce31abf7a88fc9c8764b7873a3f5362c6224baba`，566 payload。主控新复制后从 /tmp 一次执行已读 verify.py，595 项离线检查通过，含新临时目录中补丁实际应用与逆转。此执行不运行捕获产品或旧脚本。全部 229 个主库捕获文件在开始时与 worker baseline 相同；主控每个编译、测试阶段前后及真实 PTY 前后均校对这些文件，只有计划中的两个变化。

## 主库真实失败、修复与回归

主控先以当前旧产品代码新构建原生 ARM debug 二进制，再只加入新测试：`cargo +stable-aarch64-apple-darwin test --locked --offline --test tui_composer terminal_shortcut_` 实际 exit101，2 pass/3 fail。三个失败分别为 Super 修饰字符误插入、held ASCII 后 Super 误插入、菜单打开时 Ctrl-G 提示错误。原失败完整 stdout/stderr 合并日志保存。

随后应用唯一产品补丁，十组测试一次运行共 157 pass/0 fail（包含新增 5 项，不额外相加）：layout 11、layout_persistence 7、project_workspace 7、tui_approval_focus 12、tui_bentobox 16、tui_busy_diff 3、tui_command_palette 17、tui_composer 51、tui_interaction 22、tui_project_workspace 11。原生 `cargo fmt --check`、两文件 rustfmt、`cargo check --locked --offline` 及修复后新 debug build 均 exit0。既有 crossterm 警告保留，未扩改 vendor。

新测试经过生产 handle_event/handle_event_at，涵盖 Press/Repeat/Release、UTF-8、Super/Shift/Control/Alt 及既有编辑行为；TestBackend 调用真实 render_bentobox 比较当前 UI 提示与 action。合成 KeyEvent 不证明任何真实终端能发送 Super 或启用了键盘增强。

## 主控新真实终端对照

在新建的 root-pty-before/after 两个私有目录，各以当前主库新构建的二进制执行一次 200×40 PTY。主控 witness 基于完整读过的 worker v3 新建，改为新的输出目录，增加实际“command palette active”可见断言，不复用旧运行输出。原始字节流、每键输入十六进制、屏幕、持久草稿、零请求记录及命令时间均保存。

两次均 exit0：逐键 `/mo` 后菜单确实出现，旧版仍显示 Ctrl-G editor，新版隐藏；菜单中 Ctrl-G 不启动新建 dummy editor 且草稿不变。Esc 关闭后提示恢复，Unicode `界é🙂` 与 Ctrl-U/Y 往返一致，清空后 Ctrl-D 正常退出；两次 provider 请求均为 0。实际观察到退出 alternate screen 与显示 cursor 字节。没有启动真实外部编辑器，没有宣称 post-exit termios 的完全恢复。

主库旧 debug：50742152 B，SHA `b40534cd42974396646f3cdc5c5470bafdb55078a14a7d484ce743b1daa66bdd`；新 debug：50742696 B，SHA `a4b24e76689e202dc009167e7c0b95612c98a287177c804c1ca35ff4207bff80`。它们与 worker 私有编译二进制有不同身份，不能混用运行记录，也不是生产 release 或预算样本。

## 失败保留与限制

worker v1 的退出后 tcgetattr ENOTTY、v2 快速整块输入导致菜单状态不稳定及 finally timeout 遮蔽异常，均依原报告保留。v2 缺 raw 记录不补写成完整证据；主控新 witness 不受这些失败输出替代。初始未执行 verifier 的 worker -ready 包及 corrected -v2-ready 的差异继续留在原处，主控只复制最终包。

最终主库 TUI 为 639010 B / SHA `b15661a160a855ef974e82dfb40a63c8d0c5777623cd553d1d4cf2069d8b59a1`；composer 测试为 52941 B / SHA `30d9b19bc250409bac1e4254b7237f11cf8de625913d55b669a044665e89d7c1`。补丁与 worker 候选逐字相同。撤回只逆转这两处候选相对 baseline 的改动，不覆盖既有主树修改或删用户会话。

本次没有 release/冷启动预算/131/FIFO/DTrace 重跑。已有 117 首次启动预算失败继续有效。44/121 已接受项由独立 021 和 057 验收增加；本局部修复只更新 Gantt 修复记录，不把产品清单项改为接受。

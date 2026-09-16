# ZS1-123 附件路径身份修复（provisional）

实际复现成立，已做单文件修复。Unix 文件名中的反斜杠是普通字符，旧 attachment_for_path 却用 path_display 将它替换为斜杠，并把显示结果存入 InputAttachment.path。attach_value_at 随后用该字段读取大小、计算摘要、交给 Agent 暂存；turn admission 也按这个字段读取。因此两个不同文件并存时，可以成功暂存错误的另一个文件，而非仅显示有误。

基线回归在私有完整主库构建快照中实际运行，构造两个 inode 不同、内容和长度不同的普通文件：

| 实际文件 | 字节 | SHA-256 |
|---|---:|---|
| 字面反斜杠 `note\report.txt` | 26 | b8f8c46452fd38ce6f5ee97460586fa3ca711fe7ad7987be1770179152df4625 |
| 目录路径 `note/report.txt` | 40 | 419005b1b3ae7ae30bf9a7ea79282faf073ece1b8c7d8cf0cd8acdec66dd02a5 |

请求前者时，旧代码返回 accepted=true，但 receipt、pending reference、实际读取字节及 submit 后的附件元数据均指向后者（40 字节）。基线测试实际退出 101，0 passed / 1 failed，原日志完整保留在 baseline-regression.log，SHA-256 `bafbc1c70aca96ba32aa9e485e01f95dc4eef7f11856d3b3e0ce838ef79a6580`。这不是用字符串模拟 I/O；使用实际文件系统及未替换的 Agent/ToolContext/附件入队实现。测试不启动 provider 网络请求。

修复仅将附件构造中的路径序列化改为经过验证的相对 Path 的无损 UTF-8 原生拼写；不再调用显示格式化函数，无法无损表示时拒绝。InputAttachment 的既有 String 接口和 serde_json 转义保持不变。Windows 原生反斜杠仍由 Path 作为分隔符解析；不会按 Unix 普通字符规则改写。resolve_relative 的 workspace、父目录、控制字符、长度、逐级 symlink 检查，文件类型、10 MiB 元数据上限、读取字节上限，以及 Agent 自身的再次验证均未改动。

## 冻结与范围

先冻结主库当前 src/slash_actions.rs，并在任何测试/修改前按 1–310、311–620、621–903 连续完整读取全部 903 行。时间、逐块哈希及相关调用链阅读范围见 file-read-receipt.json。diff、资源、模型、queue/tree/output owner 保持原样；diff 中类似显示路径用法不在本次附件修复范围。

- exact-before：32985 字节 / 903 行，`826536a1a7296b292be759610db1140dc8081821b22f7c488e17ae072e7d2f47`。
- exact-after：41343 字节 / 1078 行，`5912f71faa07e7f529b1c9bef875a977873c6a5e152d103e837d0fbac001cd32`。
- product.patch：9074 字节，`f84468a89294226b1d81c878ca29f2ecb1758130c2f28929ca33a43a1ab35ea0`。仅一个产品文件，运行时代码为一处路径转换，另加本文件 inline tests。
- 原 worker 文件另存 worker-before.rs（30859 字节，`a57ef675472a56b3dab3276aeeef11cbfb3d5697c4c4dc8566768246164617bd`）。交接补丁以冻结主库文件为基线，保留该基线已有的其它 owner 增量；不以旧 worker 版本覆盖主库。

旧 worker core.rs 不包含新 slash_actions.rs 所需的 tree_request/tool_output_control，故使用获准的私有主库构建上下文，未尝试修改其它产品文件来凑编译。229 个真实构建输入完整复制并校验：根目录普通文件及完整 src/tests/examples/benches/vendor/tools/.cargo（存在者），不复制 .git、运行输出或 Docs。清单、每文件大小/哈希、采集前后稳定核对见 build-context.json。测试上下文唯一改动是本产品文件；无替换模块、假 Agent 或裁剪源码测试架子。

## 实际验证

所有 Cargo 命令采用 native stable-aarch64-apple-darwin、--offline --locked、独立 CARGO_TARGET_DIR；HOME/CODEX_HOME 原样。工具版本和环境见 execution-context.json，每次原始命令、退出码和完整日志均保留。

- 基线 regression：退出 101，实际错误身份已复现。
- 候选 inline：5 个 macOS 原生测试通过，退出 0。包括双文件身份全链路、只有字面反斜杠文件时成功、引号和反斜杠的 JSON 往返、规范化相对路径/Unicode/空格、workspace/控制字符/长度/非普通文件、精确 10 MiB 边界及超限读取、内部及外部 symlink 分量拒绝。
- 原有附件 integration：3 passed / 0 failed，退出 0（构造上限、暂存后下轮消耗、取消重发携带附件）；其它 8 个非附件 integration 未运行。
- 最后仅给 Unix 专属测试 import 加 cfg(unix)，避免 Windows 测试编译出现无用导入；原有 integration 的生产代码与此最终候选相同。最终候选重新执行 5 个 inline tests，退出 0。
- Clippy --lib --test slash_actions -- -D warnings：退出 0。保留依赖 vendor/crossterm 自有的 unused_parens 警告，未修改该依赖。
- Windows 专属测试已添加，但本机未运行 Windows 编译/测试；不以 macOS 测试冒充跨平台实测。
- 单文件及候选包执行正向/逆向 apply-check 和实际应用，核对 exact-after / exact-before 及无关 core.rs sentinel；哈希与完整性脚本可离线复查。

未运行全库测试、PTY、HTTP、网络、发布或真实产品服务。未改主库、蓝图、TUI、其它 owner。70 个既有顶层 ready 包（含本轮之前 CI117 包）的全部 10136 个普通文件保持原字节；其它 tracked 文件保持原字节。

G-stage 主库只读检查实际退出 1，structural=true，缺少 ZS1-123.master.json；该失败原样保留，日志 SHA-256 `c5563d7543c41caf4a21da22d17c3b294f453ae24744fbee7440398e77726eaa`。候选只供主控独立整合验证，不宣称 ZS1-123 或 ZS1-087 已验收。

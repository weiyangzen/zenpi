# 附件保留真实路径身份 — 主控整合，3.1.20

TUI 与 headless 的 `/attach` 现在保留经过 workspace 检查的原生相对路径。Unix 中 `note\report.txt` 与 `note/report.txt` 可以是两个不同文件；原 owner 把显示格式化后的字符串用于读取、摘要、暂存及下一轮提交，导致用户选择前者却把后者发送给 provider。本次只修改 `src/slash_actions.rs` 的一处转换并加入该文件内的回归测试，BentoBox 和其它产品 owner 不变。

主控连续读取当前文件 1–300、301–605、606–903 行，完整阅读候选差异和其原始失败报告，独立核对 worker manifest 的 31 项文件大小与摘要，再在临时 Git 基线上实际正向/逆向检查和应用单文件补丁，确认 exact-before、exact-after 和无关 sentinel。主库应用后，其余采集的构建输入保持相同摘要。

## 实际入口证据

同一个 `probe.py` 分别驱动旧、新原生 debug 可执行程序。每次使用一个真实 headless 进程、一个真实 TUI PTY 进程和四次 loopback Responses HTTP 请求。两个不同 inode 的文件具有不同内容和大小；检查发送体解码后的原始字节、filename、JSONL 回执的 path/sha256/size_bytes 和两种入口的 journal 元数据。附件正文不得写入 journal，stage 不得产生 HTTP，headless 拒绝 symlink、越界和缺失路径后仍可提交已暂存的有效附件，退出恢复 alternate screen。

- 旧程序：26 项检查中 19 项通过，7 项失败。两种入口均发送了目录路径的内容，而非所选字面反斜杠文件；其 path、摘要和 journal 身份随之错误。实际进程正常退出，四次 HTTP 完成，没有夹具超时。
- 新程序：相同 26 项全部通过，仍为一个 headless、一个 TUI、四次 HTTP，terminal 正常恢复。
- 最初的一次夹具尝试没有按 slash 语法转义反斜杠，被 parser 解析为 `notereport.txt` 并拒绝，0 HTTP、未进入 TUI。该失败保留为 `before-entry.*`；随后只修正夹具的命令参数编码，才得到上述旧程序的真实缺陷证据 `before-entry-corrected.*`。不能把最初夹具失败算作产品缺陷。

## 原生验证

全部 Cargo 使用 `+stable-aarch64-apple-darwin --offline --locked`，主库真实编译上下文及默认 features，HOME/CODEX_HOME 保持原样。

- 新增 inline tests 5 项通过：双文件身份、仅字面反斜杠文件、JSON 转义、Unicode/空格、规范化相对路径、路径/类型/10 MiB 大小边界及 symlink 拒绝。
- 既有 `slash_actions` 11 项与 `tui_command_palette` 17 项通过，共计 33 项 Rust 测试；未将 worker 的重复运行累计为新增测试。
- Clippy `--lib --test slash_actions --test tui_command_palette -- -D warnings` 和单文件 rustfmt 通过。既有 vendor/crossterm 依赖警告保持原样。
- 新程序 SHA256：`d150d6ef355e15bbb32b5176a15e984f2a252d396a2c7c1625cd029dab50bb52`。旧程序：`98987fe27d6f15a6fd45ce3f35d1adbeb694edc78effb6e418ff4de3389a06cc`。
- 产品 before SHA256：`826536a1a7296b292be759610db1140dc8081821b22f7c488e17ae072e7d2f47`；after：`5912f71faa07e7f529b1c9bef875a977873c6a5e152d103e837d0fbac001cd32`；单文件补丁 9074 字节。

复现：在 zenpi 根目录运行 `python3 Docs/quality/stage1/ZS1-123/master-attachment-identity-3.1.20/probe.py --binary /absolute/path/to/zenpi --evidence /absolute/new/path/result.json`。结果对应的 `.fixture` 目录必须不存在；脚本使用仓库已有 JSONL/PTY 适配器，全部请求限定本地测试服务。

## 尚未覆盖

本次是 ZS1-123 的已验证局部修复，整项和 ZS1-087 仍未验收。Windows 专属测试存在但本机未执行；没有全库、release 或预算重跑。既有扩展超时夹具失败仍待另行整合，预算记录不能覆盖本次新二进制。`/diff` 内相似显示字符串用于 Git 路径的代码尚未修复或实测，本次不声称该路径已闭合。路径检查与实际读取之间原有竞态也没有由这处字符串修复消除。

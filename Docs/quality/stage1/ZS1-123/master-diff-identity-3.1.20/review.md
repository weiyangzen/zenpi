# ZS1-123 — 主控 /diff 原生路径身份整合

已合并 C 的原生路径修复和 A 独立发现的 Git 环境冲突修复。主库只有 `src/slash_actions.rs` 改变：41343 B/1078 行、SHA256 `5912f71faa07e7f529b1c9bef875a977873c6a5e152d103e837d0fbac001cd32` → 52674 B/1371 行、`811bde9af190b2e439542c0a89dd8e8a1f76bbcf8eb6aa3ad3b16143447ec30b`。本增量验证通过，ZS1-123 与逐文件 ZS1-087 整项仍未接受。

## 主控独立阅读与行为

主控连续完整阅读原1078行（1–280、281–560、561–840、841–1078），逐条阅读C的434行产品补丁、A的完整环境修补补丁、两个完整review及A完整离线verifier；阅读全部现有295行 slash_actions 集成测试、A独立109行矩阵，以及当前TUI/headless的实际Diff分支。最终源码由完整基底和两份已读补丁重建并精确比对 after.rs，不将有界 caller 片段冒充整个 TUI/headless 文件的完整理解。

当前 path 参数通过 Path/OsStr 直接传给 Git，Unix 字面反斜杠不再转换成目录分隔符；JSON path 同样保留身份。Git status/diff 使用 literal pathspec，显式路径保留 `--`，no-index 也加入 `--`，避免文件名成为 glob、magic 或选项。两处 Command 仅对子进程删除冲突的 GIT_GLOB_PATHSPECS / GIT_ICASE_PATHSPECS，保留父环境和实际路径策略。A在C中间版本观察两个环境实际失败101，旧产品对照通过；这些历史日志原样封存，不计作本轮新运行。

附件构造/大小/hash/重验及原附件测试未改变；workspace/symlink/控制字符限制、32个untracked前缀、64KiB diff和32KiB status界限及既有kill/wait保持。原生路径显示在Windows仍按分隔符规则处理；本轮没有Windows实测。

## 实际主库验证

- 新包离线回放 exit0：核验C37文件、A50文件、历史构建上下文和失败日志；临时Git仓库依次应用C→A，再逆序精确恢复原字节和binary sentinel。主库两patch各自先check再apply；179个其他已登记Rust/vendor/Cargo/辅助工具输入hash不变。
- 原生 offline/locked Rust：owner inline 14通过，另两个真实继承环境子进程各1通过；headless_protocol 43、slash_actions 11、tui_command_palette 17，合计85顶层通过、0失败、0忽略。内嵌子进程结果单独列出，不重复计为顶层覆盖。
- 相关Clippy -D warnings、fmt check和debug build均exit0；vendor/crossterm原有依赖警告保留，无新ignore或门限放宽。
- 新debug SHA256 `226fe376f114177d812df6eeb807550dc0ae575d77f7b6f622c608e638b35426`。旧async debug `86810a096d7a2f18c55f1b80eb5a25cd51bbb3b02cc6a2762d08b0eea1e33457`保留，精确是修复前slash owner。

## 真实TUI/headless前后对照

主控新增一个有限探针，分别对旧、新debug各运行一次，输入/脚本相同，输出目录独立。每次1个真实TUI PTY和1个真实headless进程；两者均继承glob=1和icase=1。Git setup只在新私有夹具仓库中执行，并在setup子进程清除pathspec环境，产品环境未清除，以免掩盖A修复。HOME/CODEX_HOME保持，ZENPI_HOME指向独立fixture；只发送本地/diff和shutdown/quit，没有发送provider prompt。

六组名称：tracked Unix字面反斜杠与目录counterpart、方括号与glob邻居、magic-looking文件、--output-looking文件、中文空格、untracked字面反斜杠与目录counterpart。每组真实inode不同且写入唯一selected/neighbor marker。headless断言success、准确JSON path、selected hunk且无neighbor；TUI通过真实键盘提交，核验本轮新checkpoint消息与屏幕selected hunk。保存全部输入/输出、PTY原流、逐步checkpoint、屏幕、真实Git命令及保留夹具。

旧binary完整执行后exit1：tracked反斜杠、方括号、untracked反斜杠在两个host各失败，共6个失败断言；其余9个通过。新binary exit0、15/15通过。magic case在旧binary继承glob环境下恰好通过，不伪称此组合也失败；C默认环境的magic反例为独立历史证据。

每次15项包括12个身份检查、headless shutdown、无option副作用、TUI退出/alternate-screen恢复。这个探针仅检查alternate-screen序列和exit0，没有比较完整termios，不借用126探针更强的终端结论。两次进程全部退出并reap，无活跃根进程留存。

## 未完成范围

目录/全仓收集仍跳过Git quoted untracked路径；无HEAD整仓同时存在staged-only和unstaged时仍可能只显示unstaged。前者已再次派给原A任务做单owner反例修复，后者仍需后续独立增量；这些不是应永久保留的最终体验。Git下层扫描/读管道仍无额外时间取消保证，本次只修身份与环境冲突。新release/预算、123全部交互以及087正式逐文件验收仍未完成，历史budget不覆盖此debug。原BentoBox、顶部加号目录选择、async会话浏览、长回复尾部均未改动。

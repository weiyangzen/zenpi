# ZS1-126 真实会话浏览入口 probe（prepared，尚未运行 PTY）

本包提供由主控在真正合并后的 binary 上运行的脚本。worker 本轮只完成适配器阅读、脚本语法、普通 JSONL 夹具/decoder、现有 binary 参数和本地隔离规则检查；没有启动 TUI/PTY、HTTP 或 Cargo，没有修改产品、主库或上一份实现 ready，没有接受 ZS1-126 / ZS1-085。

## 文件与阅读边界

- `probe.py`：独立标准库脚本，默认 `--mode prepare`。必须显式 `--mode run` 才创建一个真实 PTY。
- `screen_adapter.py`：从完整读过的 `tui_project_workspace_smoke.py` 精确提取 `screen_text`。原文件 475 行分 1–240、241–475 完整读；依赖 `tui_user_shell_smoke.py` 171 行完整读。完整快照及 SHA 在 `adapter-read.json`。没有导入原模块，没有调用旧 HTTP 场景或旧改写 HOME 的 helper。
- `owner-*.fragments.json`：必要 owner 接口的全文哈希及阅读片段；这些 owner 未整文件 review、未修改。TUI 的键盘/行渲染/项目切换片段绑定上一任务完整读过的 e0e509… 冻结输入。
- `check-installed/`、`offline-selfcheck/`：已经执行的 CLI-only / pure-offline 验证记录与普通夹具。它们是历史证据，不是要继续使用的 runtime fixture。
- `verify_bundle.py`：只核验逐文件哈希、阅读/提取、语法与既有离线结果，不创建 PTY、启动 binary 或重跑测试。

## 主控运行

先核验本包：

```bash
python3 /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target126-session-browser-pty-3.1.20-ready/verify_bundle.py
```

确认 `--binary` 确实是合并 A async 与 B tail 后的产物，再选择一个**不存在**的新证据目录：

```bash
python3 /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target126-session-browser-pty-3.1.20-ready/probe.py \
  --mode run \
  --binary /Users/wangweiyang/GitHub/zenpi/target/release/zenpi \
  --out /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/target126-receiver-pty-run-01 \
  --steps list,selection,search-failure,project \
  --pressure-mib 8
```

可加 `--expect-binary-sha256` 固定主控构建回执里的 SHA；不匹配立即拒绝。脚本记录并在启动前再次核对 binary 哈希，但不自行证明该 binary 包含 A/B 补丁。上述 `--out` 的父目录必须已经存在；输出目录一旦存在就拒绝覆盖。

限定复验例：另选新目录、`--steps selection` 或 `--steps list,project`。步骤按脚本固定顺序执行，各步带必要前置输入，正常退出检查始终执行。`--pressure-mib 0` 用小夹具；0..48 表示对应数量的普通文件，每个含 4 个 240 KiB turn，实际每个小于 1 MiB。运行前及运行结束核对所有 fixture `.jsonl` 总量 ≤64 MiB，最大压力仍给 owner 运行记录留余量。

`--mode check` 只创建新的独立夹具、运行 binary `--help` 和 `session inspect ... --json`；`--mode prepare` 只创建夹具并做 Python schema/预算检查。两者结果是 `prepared-not-pty-tested`，绝不能记作 PTY 通过。

## 实际输入、判据与已知缺口

| 步骤 | 输入/动作 | 判据 |
|---|---|---|
| list | 先成功 `/session search needle126`，再 `/session list` | 当前 owner pane 有 B126/C126/O126、没有 G126；最新全局文本 list 回执含 G126。owner/global 是两个不同的普通目录，不统一范围。 |
| selection | 成功搜索后 Down 选 C126；新增排序在前的普通 a.jsonl；再按 Enter | 出现 A126 后选择仍为 C126，继续有界观察；真实项目 checkpoint 的 session_path 变成 c.jsonl，证明 Enter 打开选中路径。 |
| search-failure | 保留成功搜索的 rows/cursor；在 owner 目录创建普通目录 `bad.jsonl`，提交另一有效查询 | 出现新的 `session search failed:` 回执，屏幕 rows/cursor 不变；最后只移除该夹具目录并清理失败输入。没有 FIFO、symlink、特殊文件或私有产品钩子。 |
| project | 建立另一个真实项目、打开 D126 owner、过滤到 D126（排除 E126）；返回原项目提交 race126，再键盘 `/project select` 切回另一项目 | 新项目 pane 恢复 D126，并在有限观察窗内保持；该项目新增 messages 不含旧 race126 回执。 |
| exit | 真实 `/quit` | exit 0、确实进入/离开 alternate screen、持有的 PTY slave termios 与启动前逐项一致。不是仅凭进程退出宣称终端恢复。 |

屏幕检查复建 Ratatui 的当前 cursor-addressed 单元格，只识别 pane 行 `ID turns=`，不把整个历史 PTY 字节流中曾出现过的字样当成当前状态。checkpoint/messages/screen/输入时间分别保存，成功搜索等待新的回执后才进行依赖它的键盘选择。

**扫描活跃/并行性缺口：** 真实公开接口没有 scan-start barrier。普通文件压力不保证扫描持续到第二条键盘输入，即使切换后未见串项目结果，也只能算真实入口回归。`project.scan_overlap` 固定为 `UNPROVEN`；时间戳不叫“扫描期间键盘延迟”，没有并行响应性 pass 字段。确定性慢扫描输入可达和旧 generation 拒绝仍由上一份 Rust barrier/negative-control 证据承担。最小补充复验是主控运行本脚本 project 步观察真实入口，再结合已有 Rust 屏障；若要在 PTY 证明实际重叠，需要独立可观察的 scan-start 证据，不能在产品里造本次测试后门。

**负例接口边界：** 当前 owner 的 list/inspect 会拒绝普通目录 `bad.jsonl`，已有 Rust 证据也使用这种错误。如果最终合并的 scanner 改为跳过这类目录，这一步可能无法稳定生成错误。脚本保留超时/失败，不自动换注入或重试；主控可在新的证据目录只跑其它步骤，将该负例记为未达成，并用同一夹具的离线 `session inspect` 检查说明 owner 的实际拒绝/跳过行为。不能因此把缺口改记通过。

## 隔离、预算及故障证据

所有会话、auth/config、项目、checkpoint 和用户动作都在新输出目录 `fixture/` 内。`ZENPI_HOME` 指向 fixture/initial/g；HOME/CODEX_HOME 环境值保持不变。runtime 只使用标准 release 支持的 `--backend openai` 并提供明确假凭据，避免 dev-fixtures 构建要求；从不发送模型 prompt、shell 或 provider 命令，不开 HTTP server。

真正的 PTY child 经 macOS `/usr/bin/sandbox-exec` 启动，策略文件随证据保存：禁止 network*；禁止 HOME/CODEX_HOME 下 fixture 外的 file-read-data（提供的 binary 本体例外）及 fixture 外用户写入。策略已用 `/usr/bin/true` 检查语法，用普通 fixture 文件检查允许读取，用另一个私有文件检查拒绝读取。没有为检查策略尝试 HTTP。此 probe 的 run 模式目前限定 macOS；若 sandbox 初始化或系统依赖不兼容，会失败留证，不悄悄取消隔离。

PTY 原始字节即时保存到 `pty.raw`，输入按单次动作记录到 `events.jsonl`，每个成功阶段保存当前 screen 和 checkpoint JSON；失败保留 traceback/result.json、最后 screen/checkpoint，全部 journal/config/checkpoint 原地保留。原始输出有 32 MiB 上限，普通 session 总预算 64 MiB。每次等待有截止时间（常规 12 秒、CLI 15 秒、正常退出 12 秒）；没有 rerun、预热、直到绿或隐藏 retry。观察窗内轮询是在等同一次动作，不是重新提交动作。

清理仅对本次直接 fork 的 PID 发 SIGTERM，2 秒后仍未退出才 SIGKILL，再最多 3 秒 reap；不按进程名、端口或猜测 process group 清理，不删除输出目录。provider operation journal 若出现，将结果置 failed；网络策略同时阻止实际发送。脚本只清理自己创建的坏目录，不改产品和任何历史 ready。

## 本轮实际验证

- 安装位置 `/Users/wangweiyang/GitHub/zenpi/target/release/zenpi`，当时 SHA `1d7b2e61ecc9096ad0f5e6cd3f3c03c6820cafe8371d9201de3497bb6b4e8f58`。仅 `--help` 和 `session inspect <普通 B126 fixture> --json` 各执行一次，退出 0；decoder 返回 B126、1 turn、0 recovery warnings，fixture 前后哈希不变。该二进制不代表最终合并产物。
- `python3 -m py_compile` 与 offline selfcheck 通过；selfcheck 明确拦截 PTY open/fork，验证提取函数、当前屏幕行解析、schema/预算、环境和隔离策略。隔离反例 `/bin/cat outside-fixture.txt` exit 1 / Operation not permitted 是预期拒绝，stdout/stderr/argv 均保留；其它检查 exit 0。
- 未执行 `--mode run`。真实 PTY 的所有五项运行结果仍由主控填写；本包不是通过回执。

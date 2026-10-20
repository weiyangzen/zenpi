# TUI 真实测试清单（tmux + zenpi + DeepSeek）

**规则**：每项都用 **tmux 真开 zenpi** 测，不是 `cargo test`。测完在方框里打 `x` 并记一行观察结果。
**进度就写在这个文件里**——上下文被压缩时以本文件为准。

**环境**（每次全新会话都要做，否则会复用陈旧状态）：
```sh
# 1) 凭据目录必须 0700，否则 auth_storage_failed
mkdir -p /tmp/zh && chmod 700 /tmp/zh
cp ~/.zenpi/config.toml ~/.zenpi/auth.json /tmp/zh/
# 2) 关键：把陈旧检查点挪开，否则会恢复指向已删目录的会话 → 工作区恢复失败 → 会话降级、工具消失
mkdir -p /tmp/zenpi-state.bak
cd ~/.zenpi && mv project-tabs.json layout.json project-workspace.json session.jsonl \
   project-workspace.lock auth.json.lock /tmp/zenpi-state.bak/ 2>/dev/null
# 3) 起 TUI
mkdir -p /tmp/ws && cd /tmp/ws && git init -q && echo ok > README.md
tmux new-session -d -s zenpi -x 160 -y 45 -c /tmp/ws \
  "ZENPI_HOME=/tmp/zh TERM=xterm-256color ~/.cargo/bin/zenpi"
# 读屏 / 发键
tmux capture-pane -t zenpi -p
tmux send-keys -t zenpi -l "文本"; tmux send-keys -t zenpi Enter
tmux send-keys -t zenpi Tab | C-Down | Escape | M-r | C-c
```

**判据提示**：`tmux capture-pane -t zenpi -p | tail -2` 读 footer 能直接看出当前热区。

---

## 已完成

- [x] **启动 + 渲染** — 7 窗格全部正常；Resources 跑真实 LAN 发现（6 台设备）；Shell 是真实 PTY
- [x] **真实 DeepSeek 对话** — `● 提问` → `◇ Reasoning: N bytes · collapsed` → `● TUI_OK`
- [x] **Alt-R 展开推理** — 折叠摘要 → 完整推理文本
- [x] **多轮上下文** — 模型准确回忆上一轮指令；长行自动折行正常
- [x] **Tab 切区** — Conversation → Resources → Arch → Gantt → Shell → Conversation，footer 提示逐区准确
- [x] **Esc / Ctrl-方向键** — Esc 退到"无热区"；Ctrl-Down / Ctrl-Right 移动焦点
- [x] **无文本区按键反馈** — Gantt 区按普通键显示 `Gantt 不接受普通输入 · Tab 切换区域 · Esc 返回`
- [x] **TUI 工具下发**（修复 `ed68cd4` 后）— 模型列出全部 10 个工具
- [x] **审批卡片呈现** — `Tool: write_file · WorkspaceWrite` / `Policy: 4de5f870…` / `Lease: none` / `Path: hello.txt` / `Size: 0 -> 2 bytes` / **完整 diff 预览**

## 待测

### 审批交互（卡片已在屏幕上）
- [ ] `y` 选中允许 → Enter 提交 → 文件真的被写入
- [ ] `n` 进入拒绝理由输入 → 输入理由 → Enter 提交拒绝
- [x] 裸 Enter 一按确认（当前选择 = 拒绝）— 一次按键即拒绝，`a.txt` 未创建
- [x] `r` remember → 二次确认阶段 → Enter → 允许并记住（`b.txt` 落盘）
- [ ] `Tab` / `←` `→` 在多条待审批之间切换 —— **单卡片下按键已实测**（不崩、footer 显示 `Tab next · PgUp/PgDn scroll · Esc draft`），但**多条之间未能构造**：单个 owner 一次只有一条待批（turn 内工具调用串行）。需**两个 owner 同时待批**
- [x] `↑` `↓` `PgUp` `PgDn` `Home` `End` 滚动 diff 预览 —— 用 40 行 diff 实测：`PgDn`×2 → LINE_09、`Down` → LINE_10、`Home` → LINE_01、`End` → **LINE_37–40**（到底）
- [x] `Esc` 失焦 → footer `1 approvals pending · Alt-A review`，卡片收起但请求仍待批；`Alt-A` 重新聚焦 → 卡片复现 → `n`+Enter 拒绝生效
- [x] `/` 从审批视图逃回 prompt —— **大视口（180x50）实测通过**：按 `/` 后输入框出现 `│/`，卡片失焦但保留（符合设计：unfocus 不 retire）

### 其他面板与流程
- [x] Shell 面板：`echo SHELL_OK_MARKER` → 输出 `SHELL_OK_MARKER` → 新提示符（真实 PTY）
- [x] Arch 面板：`Alt-M` → footer `Arch console focused · !cmd runs bash · text steers`，独立会话回了 `ARCH_OK`
- [x] `Ctrl-C` 单次 → footer `再按一次 Ctrl-C 强制终止 · Ctrl-D 退出`；**1200ms 窗口内**双击 → 强杀成功（隔 2 秒再按只会重新计时，不是 bug）
- [x] `Ctrl-T` 打开 `Open project folder` 模态框（含 `Path:`），`Esc` 退出
- [ ] `Ctrl-W` 空草稿时关项目 / 有草稿时删词
- [x] resize：`resize-window -x 100 -y 30` 后布局正确重排
- [x] 退出：`Ctrl-D` 干净退出（注意：**模态框开着时 Ctrl-D 无效**，要先 Esc）

### 编辑和弦（在 Conversation 区）
- [x] `Ctrl-W` 删词、`Ctrl-U` 删到行首 —— `alpha beta gamma` → `alpha beta` → 空（Conversation 区实测）
- [x] `Ctrl-A`/`Ctrl-E` 行首行尾 · `Ctrl-U`→`Ctrl-Y` 删除粘回往返 · `Ctrl-K` 删到行尾 · `Ctrl-J` 换行成两行 —— 全部实测通过
- [ ] 同一批和弦在 **Resources/Gantt/无热区** 下**不得**改草稿（本轮修复点）

---

## 已确认的坑（不是应用 bug，是测试环境的）

1. **TUI 总是恢复最近一次会话**，不看 cwd。不清检查点就会看到旧内容，误判成"修复没生效"。
2. **陈旧检查点指向已删目录**时，TUI 只打印
   `Shared project workspace could not be restored: working folder I/O`，
   然后**静默降级**——会话照常可用，但模型手上一个工具都没有。
   这一条**值得当成真问题修**：降级应当可见，或自动改用当前目录。
3. 凭据目录不是 `0700` 会直接 `auth_storage_failed` 起不来。

---

## 本轮新增结果

- [x] **审批：`y` 允许** — 卡片 → `y` → Enter → `Approval decision submitted: allow once`
      → 工具执行 → 文件真的落盘（`/tmp/ws3/hello.txt`，内容 `hi`，2 字节）
- [x] **审批：`n` 拒绝 + 理由** — `n` 进理由输入 → 输入 `not now` → Enter → 文件**未创建**，
      且模型明确尊重拒绝：*"an approval denial is a permission decision, and shell-equivalent
      workarounds would subvert it"*（它拒绝了绕道用 `run_command` 写文件）
- [x] **审批卡片内容完整** — `Tool` / `Policy` digest / `Lease` / `Path` / `Size: 0 -> 2 bytes` /
      **完整 unified diff 预览**

## 🔴 确认的 bug（未修完，已回退改动）

### B1｜工作区恢复失败 → 会话静默降级成"没有工具"

**复现**：让 `~/.zenpi/project-workspace.json` 指向一个**已被删除**的目录，然后启动 TUI。

**现象**：只打印一行
```
Shared project workspace could not be restored: working folder I/O: No such file or directory
```
然后**照常运行**——界面正常、对话正常，但**模型手上一个工具都没有**。
这是本轮最危险的一条：全程没有任何可见信号，用户会以为工具坏了。

**我尝试的修法**：在 `tui.rs` 的启动路径里，恢复失败就回退到 `std::env::current_dir()`
（`project_host.apply(&mut state, ProjectIntent::Open(cwd))`），并打一条 `Started in … instead`。
**回退本身确实触发了**（屏幕上出现了 `● Started in /private/tmp/ws4 instead`），
但立刻暴露了 B2。

### B2｜回退切工作区后，journal 没有重开

回退生效后的下一条 prompt 直接失败：
```
session: session record is invalid: session writer is stale; reopen the journal before retrying
```

所以正确的修法不只是切目录，**还要把 SessionStore 一起重开/重指向**。
两个改动必须一起做，只做前者会把"静默降级"换成"响亮地坏掉"。

**结论**：改动已**回退**（`git checkout -- src/tui.rs`）。半个修复比没有更糟——
它把一种失败换成另一种失败，而且我无法在当轮验证它真的修好了。

### 建议的修法方向

1. 恢复失败时，连同 `SessionStore` 一起重开（参考 `resume_session` / `commit_resumed_session` 的做法）；
2. 或者更保守：**不要静默继续**——直接以可操作的错误退出，让用户知道工作区没了。
   这至少不会让人以为工具是坏的。
3. 无论选哪条，**降级必须可见**。

---

## 实测补充（第三轮）

**已通过的完整清单**（tmux 真开 + 真实 DeepSeek）：启动渲染 · 真实对话 · Alt-R · 多轮上下文 ·
Tab 六区 · Esc/Ctrl-方向键 · 无文本区按键反馈 · TUI 工具下发 · 审批卡片 · **审批 y 允许** ·
**审批 n 拒绝+理由** · **审批 r remember** · **裸 Enter 一按确认** · **Shell PTY 输入** ·
**Arch 独立会话** · **Ctrl-T 目录选择器** · **resize** · **Ctrl-D 退出** · **Ctrl-W/Ctrl-U 编辑和弦**

**测试手法上踩的坑（不是应用问题）**：
1. `Ctrl-Down` 只在**草稿为空**时切焦点；有草稿时不生效——想切区得先清空草稿。
2. 草稿非空时 `Tab` 走的是 **slash 补全**，不是切区。
3. **模态框开着时 `Ctrl-D` 不会退出**（按键被模态框消费），要先 `Esc`。
4. 会话恢复会保留上次的工作区与焦点；不复位会看到旧内容、误判成 bug。

**仍未实测**：审批的多条切换 / 预览滚动 / `Esc` 失焦后 footer 的待审批提示 ·
`Ctrl-C` 与双击 `Ctrl-C` · `Ctrl-J`/`Ctrl-A`/`Ctrl-E`/`Ctrl-K`/`Ctrl-Y` 逐个 ·
编辑和弦在非文本区的隔离（已由单测 `a_key_sweep_...` 覆盖，但未在 tmux 里复验）

---

## 第四轮补充（编辑和弦与 Ctrl-C）

**全部编辑和弦实测通过**：`Ctrl-A`（行首）· `Ctrl-E`（行尾）· `Ctrl-U`（删到行首）·
`Ctrl-Y`（粘回，往返正确）· `Ctrl-K`（删到行尾）· `Ctrl-J`（换行）· `Ctrl-W`（删词）

**`Ctrl-C`**：单次显示升级提示；**1200ms 窗口内**双击强杀成功。
隔 2 秒再按只会重新计时——这不是 bug，是设计窗口。

**`Esc` / `Alt-A`**：失焦后 footer 正确显示 `1 approvals pending · Alt-A review`；
`Alt-A` 重新聚焦后卡片复现。（我早先"footer 没提示"的观察是**误报**——当时请求已被解决。）

### 一条重要的实测限制

**草稿非空时，纯键盘无法离开 Conversation 区**：`Tab` 在非空草稿下走 slash 补全而不是切区，
`Ctrl-方向键` 也只在**空草稿**时切焦点。所以"在有草稿的状态下用别的区的和弦去毁草稿"
这个 W1 修复点，**现实中要靠鼠标点别的窗格才会触发**。

tmux 里无法模拟鼠标，因此**这条只有单测覆盖**（`a_key_sweep_leaves_drafts_alone_in_every_non_text_zone`
与 `a_chord_keeps_its_global_meaning_while_a_navigation_zone_is_hot`，均通过），**未在 tmux 里复验**。

### 仍未实测

- 审批：多条待批之间的 `Tab`/`←`/`→` 切换（需要同时产生两条待批，未构造出）
- 审批：`↑`/`↓`/`PgUp`/`PgDn`/`Home`/`End` 滚动 diff 预览
- 审批：`/` 从审批视图逃回 prompt
- W1 修复点在 tmux 中的复验（见上，受限于无法模拟鼠标）

---

## 第五轮：预览滚动（通过）与两条未能验证的项

**✅ 预览滚动全部通过**（用 40 行的 diff 实测）：

| 按键 | 可见行 |
|---|---|
| 初始 | LINE_01–03 |
| `PgDn` ×2 | LINE_09–11 |
| `Down` | LINE_10–12 |
| `Home` | LINE_01–03 |
| `End` | **LINE_37–40**（到底） |

> 中途我一度以为 `End` 只到 LINE_14，**那是我抓屏太早**——补一次读到 LINE_37–40。这类"读屏早于渲染"造成的假象本轮出现多次，**下判断前要复读一次**。

### 两条未能验证的（记录原因，不是"通过"）

1. **多条待批之间的 `Tab`/`←`/`→` 切换** —— 未能构造出两条同时待批的场景。
   一个 turn 内模型是**串行**发工具调用的（第一条批完才发第二条），所以只出现一条待批。
   要构造需**两个 owner 同时待批**（比如两个项目标签各挂一条），或让模型在一条消息里发多个 tool call。
2. **`/` 从审批视图逃回 prompt** —— 小视口（120x34）下 `Proposed change` 不在可视区，
   我的字符串探针探不到卡片内容，无法判定 `/` 是打到卡片还是打到 prompt。
   建议**用大视口（160x45+）重测**，或直接看卡片是否消失。

### 关于"视图未聚焦"

实测中观察到：一条审批**已解决**之后再来的新请求，卡片**不会自动聚焦**
（footer 显示 `1 approvals pending · Alt-A review`），需要用户按 `Alt-A`。
这与 `present_approval` 只在"首个请求且无 picker/browser"时自动聚焦一致。
**未判定这是否符合预期**——记下来供判断。

---

## 第六轮：`/` 已验证；多条待批仍无法构造

**✅ `/` 从审批视图逃回 prompt —— 通过**（需 **大视口 180x50**；120x34 下卡片内容不在可视区，探针探不到）

按 `/` 后：
- 输入框出现 `│/` → **确认逃到了 prompt**
- 卡片**仍然可见但已失焦**（`unfocus` 不 `retire`，请求仍待批）——符合 `approval_key` 里 `Char('/')` 的既有实现

### 多条待批为什么构造不出来（确认，不是"没试"）

我让模型在**同一条消息里发两个 `write_file`**，结果仍然**只有一条待批**：
`turn` 内工具调用是**串行**的——第一条批完才发第二条。

所以 `Tab`/`←`/`→` 的**多条切换**只能靠**两个 owner 同时待批**来触发，可能的构造方式：

1. **两个项目标签**：在项目 A 挂一条待批 → `Ctrl-T` 打开另一个目录成为项目 B → 在 B 再挂一条。
   这正是 **W3b（多 owner 审批归属）** 要覆盖的场景，值得单独排一次测试。
2. 或让模型的单个 assistant 消息里带**多个 tool call**（取决于该 provider 是否会合批）。

**单卡片下的按键已实测**：`Alt-A` 聚焦 → `Tab`/`Right`/`Left` 不崩，footer 正确切到
`Tab next · PgUp/PgDn scroll · Esc draft · …`。**能验的是"按键通"，不能验的是"多条之间切得对"。**

# zenpi TUI 现状与 ratatui 集成要点

本文只做两件事：**说清 TUI 现在的样子**（含已知的交互缺陷），以及**ratatui 0.30 里哪些东西值得集成、
代价是什么**。不含已实施改动；实施另开提交。

---

## 1. 现状要点

### 1.1 规模与形态

| 文件 | 行数 | 职责 |
|---|---|---|
| `src/tui.rs` | **21,636** | 单文件：状态、按键分发、布局、渲染、宿主导入 |
| `src/layout.rs` | 1,617 | BentoBox 布局模型（窗格、比例、折叠、焦点） |
| `src/render.rs` | 1,858 | 渲染辅助（多为通用渲染原语） |
| `src/view_model.rs` | 1,732 | 视图模型（Zone、热区、状态投影） |

`tui.rs` 21k 行是**最显眼的结构性负担**：按键分发、焦点、布局、渲染、项目标签、草稿、审批展示
全在一个文件里，改一处要在 2 万行里找上下文。

### 1.2 用了 ratatui 的什么（实测）

```rust
// src/tui.rs:32 —— 全部 widget 引用就这一行
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
```

- 用到：`text::`、`backend::`、`style::`、`widgets::`、`layout::`。
- **窗格是用 `Block` widget 画的**（2026-09-23 更正）：全文有 20+ 处 `Block::default()`，
  bentobox 的每个窗格（Conversation / Prompt / Arch / Arch prompt / Shell / Resources / Gantt）
  都是 `Block::default().borders(Borders::ALL).title(..)`。`widgets::` 只有一行 import
  是因为 Rust 把 5 个类型写在一行里，**不代表 widget 用得少**——原文此处判断有误。
- `Terminal::new(...)` 直接构造（`tui.rs:16557`、`18411`），**没有用 `ratatui::init()/restore()/run()`**，
  自研了 `TerminalGuard` 做进入/还原。
- 测试用 `TestBackend`（`tui.rs:20211+`）+ pty 冒烟（`tools/stage1_host_smoke.py`，14 个用例）。

**含义**：这个项目只把 ratatui 当"帧缓冲 + 少量基础件"用。ratatui 生态里最省事的那一层
（widget 组合、边框合并、现成的滚动/列表/输入框）基本没吃到，而这正是交互缺陷高发的地方。

### 1.3 自研的关键概念（集成时必须保留的语义）

| 概念 | 位置 | 语义 |
|---|---|---|
| `TuiState` | `tui.rs` | 单一可变状态；`handle_key` 返回 `TuiAction` |
| `TuiAction` | `tui.rs:683` | 输入 → 动作的显式结果，宿主据此执行 |
| `HotZone` | `tui.rs:5400+` | 六态热区（ZS1-177）；非 Conversation 区域丢弃普通按键 |
| `LayoutModel` | `layout.rs:934` | 窗格比例/折叠/`focused`；**是用户数据，要可持久化、忠实往返** |
| `RenderScheduler` | `tui.rs` | 合并重绘（resize 拖动、流式 chunk 不逐条绘制） |
| `BackgroundRunner` + `CancellationToken` | `runtime.rs:237/56` | 后台任务与取消；登录/资源/甘特复用 |
| 项目标签与草稿 | `tui.rs` 3073+ | 每项目独立 layout/transcript/draft，250ms 节流写盘 |

**不可动的约束**（已由测试固定）：`LayoutModel` 的往返必须逐字节忠实；
`handle_key` 必须保持"输入 → 显式动作"的可测形状。

---

## 2. 已知的交互/使用缺陷

### 2.1 已修

- **切项目后无法输入**：`focused: None` 的 layout 会静默吞掉所有普通按键（ZS1-177 回归）。
  已修在用户动作处（select/open 分发 + 启动一次），并保证恢复路径仍是忠实数据往返。

### 2.2 当前红（**归属上游 `c6f110a`**，不是本分支引入）

| 目标 | 用例 |
|---|---|
| `tests/tui_preview_graphemes.rs` | `approval_preview_keeps_character_after_wide_emoji_visible_without_allowing` —— Enter 未返回 `RespondApproval{allow:false}`（上游加了"分阶段确认"，Enter 语义变了） |
| `tests/headless_project_workspace.rs` | 1 条 |

判据：这两个文件最后都被 `c6f110a`（上游最新提交）改过；本分支对 `src/tui.rs` 的净改动只有 31 行，
且不涉及审批按键。上游 `main` 的 CI 在合并前就是红的。

### 2.3 结构性成因（判断，不是结论）

1. **手写绘制 + 手写命中测试**：没有 widget 抽象，焦点/热区/命中逻辑分散在 `handle_key` 与渲染之间，
   两边一旦不同步就出现"看得见但点不到"或"按了没反应"。
2. **21k 单文件**：按键分支与状态字段混在一起，改一个交互要跨几千行，回归靠 pty 冒烟兜底。
3. **静默丢弃**：`HotZone::None` 时普通按键被丢弃且无提示（本轮只修了"不该进入该状态"，
   没有修"进入了也该有反馈"）。

---

## 3. ratatui 0.30 可集成的要点

0.30 把单体 crate 拆成了 workspace：
`ratatui`（facade，应用继续用它）/ `ratatui-core`（稳定原语）/ `ratatui-widgets`（内置 widget）/
`ratatui-crossterm`·`ratatui-termion`·`ratatui-termwiz`（后端）/ `ratatui-macros`（宏）。

> **下表的粗体项已在 2026-09-23 逐条复核，结论见 §6：三项都不适用。** 保留原表以便对照。

| 能力 | 能解决本文哪一条问题 | 风险 / 代价 |
|---|---|---|
| ~~**`ratatui::run()` / `init()` / `restore()` + `DefaultTerminal`**~~ | 1.2 的自研 `TerminalGuard`；终端进入/还原的边角（panic 路径、raw mode 残留） | **不适用**：现有 `TerminalGuard` 职责更多（见 §6.2），换掉是功能倒退 |
| ~~**`ratatui-macros`**：`span!` `line!` `text!` `constraint!` `layout!`~~ | 1.1/2.3 的手写绘制：布局与文本拼装更短、更难写错 | **不适用**：纯语法糖；且 ratatui 0.30.2 要求 `ratatui-macros 0.7.2`，本地缓存只有 0.6.0，需联网拉取 |
| ~~**边框合并 `MergeStrategy`**~~ | BentoBox 相邻窗格的边框重复绘制（视觉噪音，也易与命中区域不一致） | **不适用**：窗格 rect 由布局保证互不重叠，合并永远不会触发（见 §6.1） |
| **Canvas 新 marker**：`Quadrant`/`Sextant`/`Octant` | 甘特/资源图的粒度 | 低。仅当那些视图想更细 |
| **`ScrollbarState::get_position()`、`LineGauge` 自定义符号、`Tabs::width`、`List::highlight_symbol` 收 `Into<Line>`** | 资源/列表/标签条的零散交互与显示 | 低。逐处替换 |
| **`ratatui-core` 独立** | 把 `view_model`/`layout` 这类纯逻辑与 widget 解耦，降低 `tui.rs` 的编译与耦合 | 中。要拆 `tui.rs` 才吃得到 |
| **`default-features = false` 现状** | —— | **先确认**：当前只开了 `crossterm`，`all-widgets`/`layout-cache` 等是否可用要实测，否则会写出"用了不存在的 widget" |

**版本与环境**：0.30 用 Edition 2024、MSRV 1.88 —— 与本项目 MSRV 一致（`Cargo.toml` `rust-version = "1.88"`）。
应用侧继续 `use ratatui::...` 即可，拆分对我们是透明的。

**明确不建议**：为了用 widget 而整体重写渲染层。现有 `LayoutModel` 的忠实往返、六态热区、
`TuiAction` 的可测形状都有测试固定，重写会把这些语义一起冲掉。

---

## 4. 建议的集成顺序

1. **先修红**：`tui_preview_graphemes` 与 `headless_project_workspace` 那两条（先判是上游分阶段确认的
   语义变更要更新测试，还是实现缺了反馈）。红着做集成等于在流沙上盖楼。
2. **低风险替换**：`ratatui-macros` + `DefaultTerminal`/`init`/`restore`，逐处、可回退。
3. **边框合并**：BentoBox 视觉与命中区域一致性，配 `TestBackend` 快照测试。
4. **静默丢弃要有反馈**：进入无热区时给出可见提示，而不是让按键消失。
5. **再谈拆文件**：把按键分发/布局/渲染从 `tui.rs` 里分出去，才谈得上吃 `ratatui-core` 的模块化收益。

---

## 5. 集成前必须先确认

- `cargo tree -e features -p ratatui` 里实际开了哪些 feature（当前 `default-features = false`）。
- CI 的 `clippy --all-targets --all-features -- -D warnings` 对新增 widget 用法的容忍度（当前已是红的）。
- 版本已实测锁定：`ratatui 0.30.2` / `ratatui-core 0.1.2` / `ratatui-widgets 0.3.2` / `ratatui-crossterm 0.1.2`（`Cargo.lock`）。**0.30 已完成拆分**，说明下面那些 widget 能力在本仓库里是可用的，`default-features = false` 关掉的是别的。
- 但 `features = ["crossterm"]` 之外还开着什么，仍要 `cargo tree -e features -p ratatui` 实测后再动手。
- pty 冒烟（`tools/stage1_host_smoke.py`）是这套 UI 唯一的端到端网，任何渲染改动都要用它复核。

---

## 6. 复核结论（2026-09-23）：三项推荐都不适用

§3 表格里三处"低风险可集成"逐条查过源码后**全部不成立**。记在这里，免得后来人照着做。

### 6.1 边框合并 `MergeStrategy` —— 结构性不适用

API 确实存在：`ratatui-widgets 0.3.2` 的 `Block::merge_borders(MergeStrategy)`，
`ratatui::symbols::merge::MergeStrategy` 经 facade 可达（`ratatui-0.30.2/src/lib.rs:517`
re-export 了 `symbols`），**零新依赖**。

但它**只在同一个 cell 被画两次时才生效**。而 zenpi 的布局保证窗格 rect 互不相交——
`src/layout.rs` 里就有 `LayoutSnapshot::visible_rects_non_overlapping()`，这是被测试钉住的不变量。
渲染帧里看到的 `││` / `┘└` 是**两个相邻 cell**各画各的，不是重叠。

要用上它，就得让窗格 rect 彼此重叠一列/一行，那会改动每一个坐标假设：命中测试、
`block.inner()`、Shell 的 `rows/cols`（`tui.rs` 里 `pane.rect.height - 2`）、
IME 光标锚点（`pane.rect.x + 1`）……正是 §3 结尾自己警告过"不要整体重写渲染层"的事。
**结论：不做。**

### 6.2 `DefaultTerminal` / `init()` / `restore()` —— 会功能倒退

现有 `TerminalGuard`（`src/tui.rs` 的 `struct TerminalGuard`）比 `ratatui::init()` 多做三件事：

1. **`terminal_signals::SignalGuard::install()`** —— 异常退出时还原终端。`init()` 没有。
2. **`EnableBracketedPaste` + `EnableMouseCapture`** —— `init()` 只做 raw mode + alternate screen。
3. **`external_editor::TerminalState::capture()`** —— 为 Ctrl-G 外部编辑器保存终端状态。

换掉 `TerminalGuard` 等于把这三项一起丢掉。**结论：不做。**

### 6.3 `ratatui-macros` —— 纯语法糖 + 要联网拉新版本

`ratatui 0.30.2` 的 `macros` feature 依赖的是 **`ratatui-macros 0.7.2`**
（`~/.cargo/registry/src/*/ratatui-0.30.2/Cargo.toml:233`），而本地缓存里只有 `0.6.0`。
本项目全用 `--locked --offline` 构建，启用它需要先联网 fetch 一次。

它提供的 `span!` / `line!` / `text!` / `constraint!` / `layout!` 是**纯语法糖**，
不修任何缺陷；替换会触及大量调用点。**结论：不做**（除非将来有别的理由动布局代码）。

### 6.4 那还剩下什么

§4 建议顺序里，真正修掉缺陷的是第 1 项（先修红）与第 4 项（静默丢弃要有反馈），
两项都已完成；第 5 项（拆文件）仍是最后再谈。
**渲染层的 ratatui 集成没有可做的项**——这套 UI 已经在用 widget，缺的不是集成度。

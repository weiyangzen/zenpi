# ZS1-302 — paste_burst.rs 完整单文件 learn 复核

状态：worker 候选（self_tested，主控 G-FILE/hash/chunks/源测试/操作映射 待审）。唯一 owner：`Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/paste_burst.rs_learn.md`。本轮只读复核并重写本报告；不修改 Codex 源、不修改 zenpi 主库、不宣称 zenpi 已实现这些能力。

## 来源、读取与 hash

唯一正式源 `codex-rs/tui/src/bottom_pane/paste_burst.rs`，Codex 参考仓 `/Users/mac/GitHub/codex`，冻结 revision `b3b3d262787f4902a7449f17d793241a34d311ad`。24594 bytes / 572 行 / SHA256 `c80558dfd3cbde437f2fec9a855b01a3180542a53bb135dc92ce7c81299f1ee7`，与冻结蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 第 432 行及 `Docs/learn/stage1_pi_mono/references/codex/source_manifest.tsv` 的 ZS1-302 行逐项一致。源远小于 256 KiB，单连续 byte chunk `[0,24594)` 已按 1–572 一次读至 EOF（第 572 行文件结束）。本报告不含源文件拷贝。

## 模块职责与状态

`PasteBurst`（171–180 行）是 `ChatComposer` 的纯状态机，把**无 bracketed paste** 终端上快速到达的普通字符流判定为“显式粘贴”或“普通输入”。模块 doc（1–146）明确其契约：模块自身不修改 textarea，只产出分类决策，由调用方执行文本插入。字段：

- `last_plain_char_time: Option<Instant>`：最近一个普通字符时间，用于 burst 间隔判定与 flush 超时。
- `consecutive_plain_char_burst: u16`：连续快速普通字符计数。
- `burst_window_until: Option<Instant>`：Enter 抑制窗口（“Enter 插入换行”）截止时间，其寿命长于 buffer。
- `buffer: String`：待作为一次 paste 输出的累计文本；非空即视为“处于 burst 上下文”。
- `active: bool`：是否仍在主动接收 burst 字符。
- `pending_first_char: Option<(char, Instant)>`：被短暂扣住的首个 ASCII 字符，用于闪烁抑制；调用方在手握期间不得渲染该字符。

辅助类型：`CharDecision`（182–192）为 `BeginBuffer { retro_chars: u16 }`、`BufferAppend`、`RetainFirstChar`、`BeginBufferFromPending`；`RetroGrab`（194–197）为 `{ start_byte: usize, grabbed: String }`；`FlushResult`（199–203）为 `Paste(String)`、`Typed(char)`、`None`。仅依赖 `std::time::{Duration, Instant}`（148–149），无 shell、网络、文件、线程或 agent 调用。

## 输入状态机与决策

| 源行 | API | 行为 |
|---|---|---|
| 212–214 | `recommended_flush_delay` | 返回 `PASTE_BURST_CHAR_INTERVAL + 1ms`，供测试/UI tick 跨过阈值 |
| 216–219 | `recommended_active_flush_delay` | `cfg(test)`：`PASTE_BURST_ACTIVE_IDLE_TIMEOUT + 1ms` |
| 222–252 | `on_plain_char(ch, now)` | ASCII 主入口。先 `note_plain_char`；`active` 时刷新窗口并返回 `BufferAppend`；已有 pending 且与 held 间隔 ≤ 阈值时 `active=true`、把 held 推入 buffer、返回 `BeginBufferFromPending`；连续计数 ≥ 3 时返回 `BeginBuffer { retro_chars: count-1 }`；否则保存 `pending_first_char` 并返回 `RetainFirstChar` |
| 260–275 | `on_plain_char_no_hold(now)` | 非 ASCII/IME 入口，返回 `Option`，**只会** `BufferAppend`/`BeginBuffer`，绝不 `RetainFirstChar`；不检查 pending 分支 |
| 277–286 | `note_plain_char(now)` | 若与上次间隔 ≤ 阈值则计数 `+1`，否则重置为 1；更新时间戳 |
| 387–407 | `decide_begin_buffer(now, before, retro_chars)` | 见“retro-capture”节 |
| 359–362 | `append_char_to_buffer(ch, now)` | `buffer.push(ch)` 并刷新 Enter 窗口 |
| 367–374 | `try_append_char_if_active(ch, now)` | `active || !buffer.is_empty()` 时追加并返回 true |
| 328–336 | `append_newline_if_active(now)` | `is_active()` 时把 `'\n'` 追加进 buffer 并刷新窗口，返回 true |
| 339–342 | `newline_should_insert_instead_of_submit(now)` | `is_active()` 或 `now <= burst_window_until` 时返回 true（Enter 应插换行而非提交） |
| 345–347 | `extend_window(now)` | 刷新 Enter 抑制窗口 |
| 350–356 | `begin_with_retro_grabbed(grabbed, now)` | 非空则写入 buffer；`active=true`；刷新窗口 |
| 445–452 | `clear_after_explicit_paste` | 彻底清理：buffer、计数、时间、窗口、active、pending |

## 时序模型与 flush

阈值常量（151–169）：

| 常量 | 非 Windows | Windows |
|---|---|---|
| `PASTE_BURST_MIN_CHARS` | 3 | 3 |
| `PASTE_ENTER_SUPPRESS_WINDOW` | 120 ms | 120 ms |
| `PASTE_BURST_CHAR_INTERVAL` | 8 ms | 30 ms |
| `PASTE_BURST_ACTIVE_IDLE_TIMEOUT` | 8 ms | 60 ms |

`flush_if_due(now)`（297–321）按 `is_active_internal()` 选取 idle 或 char-interval 超时，且用**严格 `>`** 比较（所以测试/tick 必须至少多跨 1ms）；同时 `is_active_internal()` 为真时清 `active`、`std::mem::take` buffer 并返回 `FlushResult::Paste`；仅超时但只握有 pending 首字符时返回 `FlushResult::Typed`；否则 `None`。`is_active()`（437–439）= `active || !buffer.is_empty() || pending_first_char.is_some()`；`is_active_internal()`（441–443）= `active || !buffer.is_empty()`。

Enter 语义由两段构成：`append_newline_if_active`（328–336）把 Enter 变为 buffer 内换行；`newline_should_insert_instead_of_submit`（339–342）用 `burst_window_until` 让“稍晚一点”的 Enter 仍插入换行，避免误提交。

## retro-capture 与 UTF-8 边界

`decide_begin_buffer`（387–407）针对“已按普通输入插入、随后才判定为 paste”的场景：调用 `retro_start_index(before, retro_chars)`（455–465）把**字符数**换算为 UTF-8 byte 起点，抓取 `before[start_byte..]`；满足 `looks_pastey`（含空白或 ≥ 16 个字符）才 `begin_with_retro_grabbed` 并返回 `RetroGrab`，否则 `None` 由调用方回退普通插入。retro 以字符计数表达（`BeginBuffer { retro_chars }`），调用方必须先把游标夹到 char boundary，保证 `start_byte..cursor` 是合法 UTF-8。ASCII 路径通常走 `RetainFirstChar → BeginBufferFromPending`，无需 retro；retro 主要服务非 ASCII/IME 与 retro-grab 场景。

## 清理与 flush 的差异（不可互换）

- `flush_before_modified_input`（410–420）：在应用 Ctrl/Alt/方向键等非字符输入前，把 buffer（以及 pending 首字符）作为 paste 文本交出，并清 `active`。
- `clear_window_after_non_char`（426–432）：只清分类窗口、时间戳、`active` 与 pending，**不清 buffer**；文档明确要求调用方先 flush，否则 `flush_if_due` 在下一个普通字符更新时间戳前不会吐出非空 buffer。
- `clear_after_explicit_paste`（445–452）：显式粘贴后彻底清理，防止旧 burst 残留字符泄漏到后续输入。

## 源测试清单（5 个，全部未执行）

`#[cfg(test)] mod tests`（467–572）使用 `pretty_assertions::assert_eq`，共 5 个 `#[test]`：

| 源行 | 测试 | 固定行为 |
|---|---|---|
| 474–486 | `ascii_first_char_is_held_then_flushes_as_typed` | 单个快速 ASCII 首字符被握起，超时后按普通 typed 输出，且 `!is_active()` |
| 490–511 | `ascii_two_fast_chars_start_buffer_from_pending_and_flush_as_paste` | 两个快速 ASCII 从 pending 开始缓冲，最终 `Paste("ab")` |
| 515–526 | `flush_before_modified_input_includes_pending_first_char` | 非字符输入前 flush 会把 pending 首字符一并交出（`Some("a")`） |
| 530–544 | `decide_begin_buffer_only_triggers_for_pastey_prefixes` | 短前缀 `"ab"` 不触发；含空格 `"a b"` 触发，`start_byte==1`、`grabbed==" b"` |
| 548–571 | `newline_suppression_window_outlives_buffer_flush` | buffer flush 后 Enter 抑制窗口仍在，越过 `PASTE_ENTER_SUPPRESS_WINDOW` 后失效 |

未运行任何测试；测试存在不等于通过，不能据此宣称真实终端时序、IME、跨平台 cfg 或像素行为已验证。

## 操作映射到 zenpi

“源事实”列来自本文件；“zenpi 现状”列为对 `src/tui.rs` 的静态阅读（引 `Docs/stage_1_v3_pi_mono_blueprint.md` ZS1-122 第 469 行、ZS1-129 第 528 行），仅作理解映射，**不表示已通过验收**。

| 能力 | 源事实（Codex） | zenpi 现状（静态阅读） |
|---|---|---|
| burst 状态机 | `PasteBurst` 字段 `buffer/active/pending_first_char/时间戳/窗口`（171–180） | `OrdinaryPasteBurst { buffer, last_input, suppress_until, active, rejected }`（`src/tui.rs:2339–2346`） |
| 阈值 | 8/30ms 间隔、8/60ms idle、120ms Enter 窗口、min 3（151–169） | `ORDINARY_PASTE_INTERVAL` 8/30、`ORDINARY_PASTE_ACTIVE_IDLE` 8/60、`ORDINARY_PASTE_ENTER_WINDOW` 120（`src/tui.rs:2330–2337`），数值一致 |
| flush/超时 | `flush_if_due` 严格 `>`（297–321） | `flush_ordinary_paste`（`src/tui.rs:5746–5757`）用 `saturating_duration_since(..) > timeout`；`ordinary_paste_timeout`（5759–5765）按 `active` 选 idle/interval |
| tick 依赖 | 定时器依赖调用方 tick，tick 停止则延迟到下次 flush（doc 31–37） | 主机用 `ordinary_paste_wait`（5767–5777）把唤醒时间收紧到 `timeout+1ms` |
| 显式粘贴优先 | 模块只处理无 bracketed paste 路径；真实 paste 由上层 `handle_paste` 完成 | `handle_event_at` 显式处理 `Event::Paste(text)`（`src/tui.rs:5913–5933`），与 burst 路径分离 |
| 非 ASCII/IME 不扣留 | `on_plain_char_no_hold` 永不 `RetainFirstChar`（260–275） | 非 ASCII 且非 fast 时立即 `insert_text`，注释明确“不扣留孤立 IME commit”（`src/tui.rs:5854–5864`） |
| Enter 抑制 | `newline_should_insert_instead_of_submit` + 窗口（339–342） | Enter 在 `active`/fast 非空 buffer/`suppress_until` 内转 `'\n'` 入 buffer（`src/tui.rs:5867–5878`） |
| 有界缓冲 | 无显式上限，内存/文本上限由上层 composer/协议限制（见缺口） | 追加前按 `MAX_MESSAGE_BYTES = 256KiB`（`src/tui.rs:44`）判定，超限清 buffer 并置 `rejected`，flush 时给用户状态而非污染草稿（`buffer_ordinary_char` 5807–5822、`flush_ordinary_paste_buffer` 5779–5800） |
| 结束/清理 | `flush_before_modified_input` 与 `clear_window_after_non_char` 分离（410–432） | 非 burst 事件触发 `finish_ordinary_paste` 并重置状态（`src/tui.rs:5881–5883`、5802–5805） |
| retro-capture | `decide_begin_buffer` 回捞已插入字符，按空白/≥16 字符判定（387–407） | **未实现等价 retro-grab**：无字符回捞逻辑，改以显式 `Event::Paste` + 有界 buffer 覆盖；大段粘贴走 `insert_paste` 的 `PasteFold` 折叠（`src/tui.rs:6636`） |

测试/证据映射：zenpi 侧相关断言见 `src/tui.rs:1638` `rejected_paste_restores_metadata_without_overriding_new_terminal_input`，以及 `tools/tui_composer_smoke.py:682` 的 `bracketed_paste_never_executes_shortcuts_or_commands`。这些是 zenpi 自有测试，**本轮未运行**，不能作为 Codex 侧行为的证明。

## 缺口与建议（均为静态映射，未运行）

- **Enter 与扣留首字符交错**：`append_newline_if_active` 只按 `is_active()` 判定，会把换行写入空 buffer 却不消费 `pending_first_char`；若随后 held 落入同一 buffer，先前的 `'\n'` 与 held 字符顺序可能与真实输入不符（源 328–336 与 232–241 组合）。建议对“单 pending + 立即 Enter + 后续快字符”补正负测试。
- **retro 判定为启发式**：`looks_pastey` 仅凭空白或 ≥16 字符（395–396），短词/短 URL 不触发；retro 以字符数换算 byte 起点，调用方必须夹 char boundary，否则切片可能落在多字节中间（77–83）。IME、组合字符、grapheme 不能只按 char 计数推断。
- **无显式容量上限**：模块自身不设 buffer 上限（`append_char_to_buffer` 359–362），长 burst 的内存/文本预算依赖上层 composer/协议；zenpi 已用 `MAX_MESSAGE_BYTES` + `rejected` 补足，但需验证拒绝后状态不被后续 flush 覆盖。
- **tick 停止即延迟**：flush 只在调用方 tick 时发生（doc 31–37）；zenpi 用 `ordinary_paste_wait` 收紧唤醒，仍需验证长时间无事件时 pending 不会被无限期扣留。
- **建议 zenpi 覆盖**：慢速 ASCII、快速 ASCII、非 ASCII IME、Enter 混入（含抑制窗口边界）、显式 `Event::Paste` 与 burst 并发、禁用/取消与 focus 切换后的残留、超长 burst 拒绝、以及 retro 差异导致的“已插入前缀再判定为 paste”场景。
- 与显式 bracketed paste 的关系：burst 是**回退**路径；不应把 burst 判定当作真实 paste 边界的证明，显式 paste event 应始终优先。

## 边界声明

本轮未运行 Cargo、产品、PTY、网络或旧 runner，未修改源码或主库蓝图/索引/验收勾选，未复制整份源。报告只支持 Codex paste-burst 状态机、时序阈值与操作映射的理解；不支持任何“zenpi 已实现上述能力”或“测试已通过”的结论。

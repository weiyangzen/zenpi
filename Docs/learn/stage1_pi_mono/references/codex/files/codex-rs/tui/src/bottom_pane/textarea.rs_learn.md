# ZS1-301 — textarea.rs 完整单文件 learn 复核

状态：worker 候选（self_tested，主控 G-FILE/hash/chunks/源测试/操作映射 待审）。唯一 owner：`Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/textarea.rs_learn.md`。本轮只读复核并重写本报告；不修改 Codex 源、不修改 zenpi 主库、不宣称 zenpi 已实现这些能力。

## 来源、读取与 hash

唯一正式源 `codex-rs/tui/src/bottom_pane/textarea.rs`，Codex 参考仓 `/Users/mac/GitHub/codex`，冻结 revision `b3b3d262787f4902a7449f17d793241a34d311ad`。91601 bytes / 2449 行 / SHA256 `8cb5241c9e818210bfff63975e703f45a25a3da91bdb395667893903f7cb4b45`，与冻结蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 第 431 行一致。源小于 256 KiB，单连续 byte chunk `[0,91601)` 已按 1–1200、1201–2388、2389–2449 分段读取至 EOF（第 2449 行文件结束）。本报告不含源文件拷贝。

## 模块职责与状态

`TextArea`（62–70 行）是 `ChatComposer` 底层的可编辑缓冲区，字段为：`text: String`、`cursor_pos: usize`、`wrap_cache: RefCell<Option<WrapCache>>`、`preferred_col: Option<usize>`、`elements: Vec<TextElement>`、`next_element_id: u64`、`kill_buffer: String`。`TextElement`（40–44）为 `{ id: u64, range: Range<usize>, name: Option<String> }`；`TextAreaState`（78–82）只保存 `scroll: u16`；`WrapCache`（72–76）保存 `width` 与换行后各行 byte `Range`。

模块 doc（1–11）明确契约：整缓冲区替换 API 只重建可见草稿状态（清空元素范围、失效光标/换行缓存），**故意保留 kill buffer**，使 submit、slash dispatch 等合成清空之后 `Ctrl+Y` 仍可恢复最近一次 `Ctrl+K`。本模块只保留单条 kill，不实现 Emacs 多级 kill ring。

## 键位与输入路由

`input`（291–537）只处理 `Press`/`Repeat`，忽略 `Release`；未识别的键仅在 `debug-logs` 特性下打日志。分派如下（S 为源行）：

| S行 | 键 | 动作 |
|---|---|---|
| 302–313 | 无修饰的 C0 字符 `^B/^F/^P/^N` | 兼容终端回退为左右上下移动 |
| 314–321 | Char(NONE\|SHIFT) | `insert_str` 插入字符 |
| 322–330 | Ctrl-J / Ctrl-M / Enter | 插入 `\n`（不提交） |
| 331–337 | Ctrl+Alt+h | `delete_backward_word` |
| 340–344 | AltGr 组合（`is_altgr`） | 作为普通字符插入 |
| 345–358 | Alt+Backspace / Backspace / Ctrl-h | 删除前一个原子边界 |
| 359–377 | Alt+Delete 或 Alt-d / Delete 或 Ctrl-d | 前向删词 / 前向删一个原子边界 |
| 379–385 | Ctrl-w | `delete_backward_word` |
| 389–402 | Alt-b / Alt-f | 移到上一词首 / 下一词尾 |
| 403–423 | Ctrl-u / Ctrl-k / Ctrl-y | kill 到行首 / kill 到行尾 / yank |
| 426–467 | Left/Right、Ctrl-b/f/p/n | 光标左右、上下 |
| 471–494 | Alt/Ctrl + Left/Right | 词导航 |
| 495–505 | Up/Down | 上下移动（优先按可视换行） |
| 506–531 | Home/Ctrl-a、End/Ctrl-e | 行首（行首处上移一行）/ 行尾（行尾处下移一行） |

该分派只产生编辑和光标结果，不提交、不排队、不发网络、不执行 shell。

## 编辑、元素与 ByteRange 不变量

- `insert_str_at`（156–165）先把插入点 `clamp_pos_for_insertion` 移出元素内部，插入后 `shift_elements`，若 `pos <= cursor` 移动光标。
- `replace_range`（167–170）先经 `expand_range_to_element_boundaries`（1110–1130）把范围扩张到完整覆盖相交元素，再 `replace_range_raw`；删除/替换后调用 `update_elements_after_replace` → `shift_elements`（1132–1155）删除被完全覆盖的元素并平移其后的元素范围。
- 光标始终被 `clamp_pos_to_char_boundary`（1056–1074）与 `clamp_pos_to_nearest_boundary`（1076–1090）约束到 UTF-8 char boundary 且不在元素内部；`prev_atomic_boundary`/`next_atomic_boundary`（1161–1211）以 grapheme cluster 为原子单位并跳过整元素。
- `beginning_of_previous_word`/`end_of_next_word`（1213–1253）以空格与 `WORD_SEPARATORS`（33 行）分词，并 `adjust_pos_out_of_elements` 跳出元素。
- 命名元素 API：`insert_element`（928–936）、`insert_named_element`（939–946，`cfg(not(target_os="linux"))`）、`replace_element_by_id`（948–961，替换后移除该 name）、`update_named_element_by_id`（966–983，保留 id 供 录音→转写→最终 复用）、`named_element_range`（986–991）。`add_element_range`（1009–1031）只标记既有范围，重复/相交范围返回 `None`；`remove_element_range`（1033–1043）按精确范围删除。
- 导出：`element_payloads`（814–819）、`text_elements`（821–835，构造 `ByteRange`）、`text_element_snapshots`（837–850）、`element_id_for_exact_range`（852–857）。元素 payload 直接取 `text[range]`，元素范围必须与文本字节偏移同步。

## kill buffer 契约

`kill_range`（634–647）把删除文本写入 `kill_buffer` 再删除；`kill_to_end_of_line`（591–606，行尾时连 `\n` 一起 kill）与 `kill_to_beginning_of_line`（608–619，行首时 kill 前一 `\n`）都经 `kill_range`。`delete_backward_word`/`delete_forward_word`（568–583）也用 `kill_range`，故词删除会覆盖 kill buffer。`yank`（626–632）在 kill buffer 为空时直接返回。`set_text_clearing_elements`/`set_text_with_elements`（103–146）明确不触碰 `kill_buffer`，测试 `kill_buffer_persists_across_set_text`（1780–1792）固定该行为。

## 光标、换行与渲染

`wrapped_lines`（1269–1288）用 `crate::wrapping::wrap_ranges` + `textwrap::WrapAlgorithm::FirstFit` 生成各行 byte 范围并缓存（宽度变化即重算）。`effective_scroll`（1295–1321）保证光标可见、内容适配时不滚动、滚动不超过 `total-area_height`。`cursor_pos_with_state`（225–236）按显示宽度计算列与屏幕行。渲染实现 `WidgetRef`（1324–1329）与 `StatefulWidgetRef`（1331–1343）；`render_lines`（1362–1390）先以默认样式写整行，再对相交元素叠加 `Color::Cyan` 前缀样式；`render_lines_masked`（1392–1410）按 char 数用掩码字符替换，不含元素样式。渲染阶段不执行命令、不读取网络。

## 源测试清单（32 个，全部未执行）

`#[cfg(test)] mod tests`（1413–2448）含 32 个 `#[test]`、`rand_grapheme`（1420–1459，覆盖换行/空格/大小写/数字/emoji/CJK/组合字符/ZWJ）与 `ta_with`（1461–1465）两个 helper。其中一个 `altgr_ctrl_alt_char_inserts_literal`（1921）在非 Windows 上 `ignore`。类别：

| 类别 | 测试 |
|---|---|
| 插入/替换/光标基准 | insert_and_replace_update_cursor_and_text、insert_str_at_clamps_to_char_boundary、set_text_clamps_cursor_to_char_boundary |
| 删除边界与元素原子性 | delete_backward_and_forward_edges、delete_forward_deletes_element_at_left_edge、delete_backward_word_and_kill_line_variants、delete_forward_word_variants、delete_forward_word_handles_atomic_elements、delete_backward_word_respects_word_separators、delete_forward_word_respects_word_separators |
| kill/yank | yank_restores_last_kill、kill_buffer_persists_across_set_text |
| 键位回退与修饰键 | control_b_and_f_move_cursor、control_b_f_fallback_control_chars_move_cursor、delete_backward_word_alt_keys、delete_backward_word_handles_narrow_no_break_space、delete_forward_word_with_without_alt_modifier、delete_forward_word_alt_d、control_h_backspace、altgr_ctrl_alt_char_inserts_literal |
| 光标移动/grapheme | cursor_left_and_right_handle_graphemes、cursor_vertical_movement_across_lines_and_bounds、home_end_and_emacs_style_home_end、end_of_line_or_down_at_end_of_text、word_navigation_helpers |
| 换行/滚动/渲染 | wrapping_and_cursor_positions、cursor_pos_with_state_basic_and_scroll_behaviors、wrapped_navigation_across_visual_lines、cursor_pos_with_state_after_movements、wrapped_navigation_with_newlines_and_spaces、wrapped_navigation_with_wide_graphemes |
| 随机不变量 | fuzz_textarea_randomized（500 例 × 60 步，断言光标不越界、光标不在元素内、元素 payload 不变、渲染不 panic、内容适配时 scroll==0） |

未运行任何测试；测试存在不等于通过，不能据此宣称真实终端时序、IME、跨平台 cfg 或像素样式已验证。

## 操作映射到 zenpi

下表的“源事实”来自本文件；“zenpi 目标”是 Zenpi 现有/拟议 owner（引 `Docs/stage_1_v3_pi_mono_blueprint.md` ZS1-122/ZS1-129，仅作理解映射，不表示已实现）。

| 能力 | 源事实 | zenpi 目标与最小判据 |
|---|---|---|
| 文本+光标+显示列三者换算 | text/cursor 为 byte 偏移，显示列用 `width()`，光标按 grapheme 原子移动（242–272、1056–1211） | `src/tui.rs`/`src/input_queue.rs` 若只有 `String`+u16 列，需显式 byte/char/col 转换；中英混排与组合字符需负例 |
| 元素/占位符与 ByteRange | 元素范围与文本字节偏移同步，插入/删除/替换扩张到元素边界（1110–1155） | mention/paste/附件占位符需要不可变快照或统一重绑定，避免删除后旧范围悬挂 |
| kill buffer | 编辑器本地单条缓存，整缓冲区替换不清除（1–11、103–146、634–647） | 提交后 `Ctrl+Y` 恢复的正确性属于该缓存契约；zenpi 提交/切 reasoning 后需验证 |
| 键位路由与回退 | 覆盖 Home/End、词移动、Backspace/Delete、Enter/newline、kill/yank 及 C0/AltGr 回退（291–537） | `tests/tui_composer.rs`、`tools/tui_composer_smoke.py` 需要正负/取消用例 |
| 换行与滚动 | wrap 缓存按宽度、`effective_scroll` 保持光标可见（1269–1321） | 长中英文、滚动到末行、resize 需真实 PTY 负例 |
| 渲染样式 | 元素叠加 `Color::Cyan`，掩码渲染不保留元素样式（1362–1410） | zenpi 若用占位符渲染，需确认样式与掩码路径不泄露/不丢失占位符 |

## 缺口与建议（均为静态映射，未运行）

若 zenpi 仅维护普通 `String` 与列号，则需补 byte/char/display-column 三者换算、元素范围与快照重绑定。建议对中英文混排、组合字符、元素跨行删除、粘贴后撤销、滚动到末行、空文本 yank、Windows AltGr、非 Windows 语音命名元素路径增加负例测试。这些是理解性建议，不是对 zenpi 现状的实现承诺。

## 边界声明

本轮未运行 Cargo、产品、PTY、网络或旧 runner，未修改源码或主库蓝图/索引/验收勾选，未复制整份源。报告只支持交互状态、元素范围与编辑语义的理解；不宣称 zenpi 已实现上述能力，也不替代主控独立源码复核。

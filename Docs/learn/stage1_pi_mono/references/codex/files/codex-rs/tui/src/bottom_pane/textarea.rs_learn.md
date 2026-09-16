# ZS1-301 — textarea.rs 交互缺口审查

源 `codex-rs/tui/src/bottom_pane/textarea.rs`：91601B / 2449L / `8cb5241c9e818210bfff63975e703f45a25a3da91bdb395667893903f7cb4b45`。已分段读取 1–1200、1201–2388、2389–2449 至 EOF。该文件是 `ChatComposer` 的底层编辑器：维护文本、光标、行列换算、命名元素和 ByteRange，处理插入/删除/替换、撤销式 kill buffer、滚动和渲染状态。

主要边界：元素范围与文本字节偏移必须同步；多字节 Unicode 光标移动按 char boundary，不能把显示列当 byte index。删除/替换会重写受影响元素范围，元素 ID 用于附件、mention、paste 和 voice placeholder 关联。`insert_element`、`insert_named_element`、`replace_element_by_id` 等 API 允许上层暂存不完整状态，调用方须在提交前重新计算范围。

Textarea 支持 Home/End、左右上下、word movement、Backspace/Delete、Enter/newline、kill-to-end 和 yank。kill buffer 是编辑器本地缓存，ChatComposer 在成功提交后有意不清除它。滚动状态根据可见行、宽度和光标位置更新；渲染阶段将命名元素转换为 styled spans，不执行命令或读取网络。

粘贴文本可整体插入并保留元素占位符；大文本的占位符替换必须按原始 ByteRange 处理，避免在 Unicode 中间切片。文本长度、换行和控制字符策略由上层 composer/协议决定，本文件的编辑 API 不能单独代表最终输入安全预算。

建议缺口：zenpi 当前 TUI 若只维护普通 String 和 u16 光标，需引入 byte/char/display-column 三者明确转换；提交、附件和 mention 需要不可变快照或统一重绑定，避免删除/撤销后旧范围悬挂。应为中英文混排、组合字符、元素跨行删除、粘贴后撤销、滚动到末行和空文本 yank 增加负例测试。该建议是静态映射，不执行测试，不修改源码。

本轮未运行 Cargo、产品、PTY、网络或旧 runner；报告只支持交互状态和范围管理的理解，不宣称 zenpi 已实现这些能力。

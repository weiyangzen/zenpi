# ZS1-300 — chat_composer.rs 全文件理解报告

状态：[_]；唯一 owner：`Docs/learn/stage1_pi_mono/references/codex/files/codex-rs/tui/src/bottom_pane/chat_composer.rs_learn.md`。源文件 374943B / 9785L / `234189c66f50c8654ef72e86ad5463eea5dd1e0c09c98f8d64c898449161b6da`，按 1–2000、2001–4000、4001–6000、6001–8000、8001–9785 连续读至 EOF；当前读取脚本已保留原始行块和 hash。当前文档只把本文件作为 Codex TUI 消费者理解，不能转译成 zenpi 的实现或安全保证。

## 文件职责与状态

`ChatComposer` 是底部聊天输入的状态机，维护 `TextArea` 文本/光标、文本元素、附件、历史、活动弹窗、slash 命令元素、技能/插件/connector mention、footer 状态、paste burst、任务运行状态、语音录音与转写占位符。`InputResult` 将提交、排队、slash dispatch、取消等动作交给上层；本文件不直接持久化会话、不实现 agent 调度，也不授予 shell 权限。

构造函数初始化配置、历史、弹窗、附件及语音状态。公开 setter 只切换功能门：skill/plugin/connector mentions、collaboration mode、connectors、fast command、personality、实时会话、音频设备、image paste、voice transcription、Windows degraded sandbox、focus、task running。`set_steer_enabled` 当前为空实现；调用者不能据此推断 steer 已启用。`input_enabled=false` 时跳过编辑并显示占位提示。

### 键盘路由

`handle_key_event` 先根据 `ActivePopup` 分派：slash popup 处理上下移动、Enter/Tab 选中命令、Esc 关闭、数字快捷提交和参数元素；file popup 处理搜索结果、路径插入和图片识别；skill popup 处理 mention 选择；无弹窗路径进入 `handle_key_event_without_popup`。每次处理后同步 popup，保证 UI 依据最新文本/光标。

普通输入经过 shortcut overlay、remote image row selection、voice space handling、paste burst 和 `handle_input_basic_with_time`。Escape、Ctrl-C、Ctrl-D、Ctrl-K、Ctrl-Y、左右/上下、Home/End、Backspace/Delete、Enter、Tab 各有独立分支。Enter 默认提交；多行输入由显式 newline 路径决定。任务运行时 Tab 请求 queue；无运行任务时 Tab 与 Enter 相同；`!` shell command 输入不会因 Tab 提交。此文件只形成用户输入事件和 `InputResult`，实际 shell 运行在外层 tool owner。

Slash command 元素在光标下通过 `/name` 识别，已知命令才提升为原子元素；带参数的 `/plan`、`/review` 复用 submission preparation，保留文本元素。`parse_slash_name`、numeric placeholders 和 custom prompts 展开都在提交前完成，输入文本 trim 后重新计算 ByteRange。

### 提交、队列与附件

`prepare_submission_text` 是提交前关键边界：展开 pending paste 占位符，按字节范围重建其他 TextElement；trim 空白并重基准范围；展开 `/prompts:` 自定义 prompt（命名或数字参数）；按 surviving placeholders 修剪本地图片。远程图片 URL 即使文本为空也作为独立附件保留。成功提交或 slash dispatch 清空 textarea，但故意保留 kill buffer，使 Ctrl-K 后的文本可在切换 reasoning 等操作后 Ctrl-Y 恢复。

`handle_submission`/`handle_submission_with_time` 记录历史并返回 queue/submit 结果；numeric slash auto-submit 走同一 paste expansion 和 attachment pruning。`attach_image` 保存本地路径，`take_recent_submission_images_with_placeholders` 将 surviving 本地图片交给上层。提交路径不会自行发网络请求或调用模型。

### 历史恢复与元素完整性

`ChatComposerHistory` 合并持久跨会话文本历史和本地 session 完整历史。持久记录只恢复纯文本；本地记录恢复 text elements、本地图片和远程 image URL。重复 Up/Down 把光标置于行尾。`trim_text_elements` 删除 trim 范围外元素并裁剪 ByteRange。`expand_pending_pastes` 以 placeholder 为 key、按出现顺序消费 pending payload，调整后续范围；空 pending 或空 elements 直接原样返回。

mention snapshot 只保留当前 `$name` 元素与其 binding path；恢复时按文本顺序重新插入 element range。`insert_selected_path` 替换光标所在 `@token`，路径含空白时加双引号（若已有双引号则不再包裹）；这是显示/参数解析便利，不是路径授权。`insert_selected_mention` 仅绑定 mention name 与 path，不能证明 path 存在或安全。

### 弹窗、文件与远程图片

slash/file/skill popup 均为本地候选列表。文件搜索结果选中后，图片路径会读取尺寸并挂本地附件；元数据失败回退为普通路径插入。远程图片渲染为不可编辑 `[Image #N]` rows；Up 在 textarea 光标 0 时进入最后 remote row，Up/Down 在 rows 间移动，Down 末行回到 textarea，Delete/Backspace 删除选中 row。远程编号从 1 开始，本地 placeholder 从 M+1 开始；删除远程 row 后本地编号重排。

### 粘贴 burst 与输入边界

Windows 等无 bracketed paste 的终端会把粘贴拆成快速 Char/Enter 事件。`PasteBurst` 暂存 ASCII 首字符以抑制闪烁，非 ASCII 首字符不延迟以保护 IME；flush 时转换为显式 `handle_paste` 或普通输入。禁用 burst 会先 flush/clear 未完成状态再按普通键处理。`handle_paste` 与 `handle_paste_image_path` 负责换行、路径规范化和 image format 识别；文本最终受 `MAX_USER_INPUT_TEXT_CHARS` 约束，过大返回用户可见错误。该限制是输入 UI 限制，不是后端 prompt/网络 payload 的完整预算。

### 语音按住说话

非 Linux 平台支持 Space hold-to-talk。支持 key release 的终端用 press/release；不支持 release 的终端用 repeated Space 作为仍按住证据：pending hold 在 700ms grace 内无 repeat 就恢复普通空格，有 repeat 才开始录音；录音期间 250ms 无 repeat 会停止并转写。开始录音插入空占位和 meter；停止后少于 1 秒直接移除 placeholder，否则显示 braille spinner，调用异步 transcription，并以 AppEvent 更新。语音线程、音频捕获和转写属于显式功能路径；本次静态阅读未启动它们。Linux cfg 下相关实现不编译，不能由当前源码推断 Linux 语音行为。

### UI 渲染与滚动

`layout_areas` 将 remote rows、textarea、status line、footer 分区；`cursor_pos`、footer helpers、`render` 系列负责高度、截断、提示和 context window。textarea 使用 ratatui Buffer 与 TextAreaState 渲染；滚动、光标列和行号依赖 textarea 内部状态。footer mode 在活动后 reset；status line、active agent label 仅作缓存，setter 返回是否变化以避免不必要 redraw。渲染层不会修改提交数据。

### 测试与未闭合边界

源文件包含 136 个 `#[test]` 声明和 172 个非测试函数；函数索引按行绑定保存。测试主要覆盖 textarea 元素范围、prompt expansion、slash parsing、history、paste burst、attachment pruning、remote image selection、mention binding、footer layout 和 voice state。没有运行任何测试。未运行意味着不能宣称键盘时序、跨平台 cfg、真实终端粘贴、音频设备、线程取消、Unicode 光标边界或渲染像素行为已通过。

消费者映射：zenpi 自己的 `src/tui.rs`、`src/headless.rs`、`src/slash.rs` 只能借鉴“输入事件→上层结果”的边界；它们不自动获得 Codex 的 popup、history、attachment、voice 或 kill-buffer 语义。Codex 的 `UserShell`/审批/工具执行在更高层，不能把 composer 中的 `!` 文本或 Tab queue 解释为已授权命令。外部报告、旧候选、ZS1-098 锁文件和其他 owner 均未借用为本报告证据。

本轮没有源码改动、没有 Cargo/网络/PTY/HTTP/release/agent runner 执行。最终报告应由 master 将唯一文件映射写入 manifest 后再接受为 `[x]`。

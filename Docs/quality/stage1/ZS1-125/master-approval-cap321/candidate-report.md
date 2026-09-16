# ZS1-125 审批预览截断修复候选

本轮只验证审批预览的可见性和滚动边界，主库未写入，等待 master 合入。

## 复现

输入树按主库当前 3.1.21 版本以 `copyfile` 复制，保留独立 mtime。审批 owner 使用真实 `ToolRegistry::approval_preview` 生成 `write_file` Diff；没有直接伪造 `ToolPreview`，也没有执行写入。40,000 字节 ASCII 新文件内容产生 `truncated:false` 的完整 owner patch；窄 8/5 列 TestBackend 复现 End 只停在 `Arguments` 前缀，尾部 `CAP_END` 不可达；84 列复现路径与 Proposed change 被重复 arguments 推出初始窗口。前版本另有一个 100,000 字节新文件尝试因 ToolCall 64 KiB 序列化上限而在 harness 构造失败，这条失败原样保留，不作为产品反例。

修复前管线：`cargo +stable-aarch64-apple-darwin test --offline --locked --target aarch64-apple-darwin --test approval_cap -- --nocapture --test-threads=1`，私有 `ZENPI_HOME`/`TMPDIR`，HOME/CODEX_HOME 未变，exit 101，**2 passed / 5 failed**。失败包括 classic/Bento End、长 tab 尾部、正常宽度 diff 可见性，以及上述非法超大调用构造失败。输出和 stderr 保存在包外 before-evidence。

## 修改

修改两个生产文件：

- `src/render.rs` 新增 `RenderedPlainWindow` 与 `render_plain_window`。它复用现有 grapheme-aware `walk_wrapped_segments`，先计算完整视觉行数，再仅保留 `[offset, offset+height)` 的可见行；offset 支持 usize，End 可跨越 `u16::MAX` 行；输入仍受 256 KiB 字节上限，返回 `input_truncated` 供 UI 明示。
- `src/tui.rs` 将审批视图 scroll 从 `u16` 提升为 `usize`，End 使用 `usize::MAX`。Diff 预览优先显示 Path、大小和 Proposed change，不再把完整 arguments 放在 Diff 前面；无 preview 的请求仍显示 arguments。owner `truncated` 或 renderer 输入截断时，底部显示 `INCOMPLETE preview · inspect source · n deny`，保持默认 deny 和 Enter 行为。request/project/turn/call 关联、draft、独立 scroll、BentoBox/classic 布局均保留。

没有修改审批 coordinator、ToolRegistry owner 校验、写入执行、草稿或项目归属逻辑。未扩大正文 inspector，也没有修改 render 的既有 grapheme 修复。

## 修复后证据

after 管线同样使用 native stable-aarch64-apple-darwin、offline locked、私有 ZENPI_HOME/TMPDIR，单次执行，exit 0，**9 passed / 0 failed**。新增覆盖：完整 owner patch 在 classic/Bento End 可达；18,000 tab 的尾部可达；正常窗口首屏显示 Path 和 Proposed change；owner 截断提示；两个审批导航保留 draft/identity/default deny；emoji/grapheme 不裁剪后续文本；window helper 可访问中间视觉行和超过 65,535 行的尾部，并明确 input cap。

## 限制

证据仅来自 TestBackend unit-style harness；没有 PTY、真实终端、HTTP、release、117/131、FIFO/DTrace 或产品运行。测试确认 renderer/TUI 逻辑和 owner preview 合成，不证明所有第三方工具或非 TestBackend 后端的视觉布局。after 仍保留 256 KiB accepted display input 上限；超过上限会明示 `Display truncated; inspect full arguments before allowing.`。完整 owner patch 在自身 64 KiB diff 上限内才可显示，owner 已标记截断时 UI 不把提示当作完整 diff。这个候选不宣称 ZS1-125 整项通过。

## 文件与身份

当前输入捕获：render.rs 59562B/SHA `9e526c42e1394cc2aa888f9627e7e0dfaf5b5e3acd0db3187c65553884d61c0a`，tui.rs 669221B/SHA `801bffa879a211fc8b1fe3f43463dd38584f7f570b870d91f0c5ba2258d79c02`。after render.rs 61553B/SHA `b98356df9020c5e6c8ec712e9c2c673265c1392cc0cee0c7d5576fc11a8d5a3a`，after tui.rs 670204B/SHA `d1c2021c7fa2259f8c4736c37314eb95f19fc8d17e0fac4e147614eb48b3012c`。测试 harness after SHA `bb1793932f42ae4fae544671259ce78878a6d0200da5e43149ead286893b9ee6`；runner SHA `caeac7ed90a937f87995776ebdf081306fa374c948575cd96a7f1ad11f0457fe`（两阶段冻结前后 runner 相同）。修改仅两生产文件加测试 harness；master 应审阅 `after-tui.patch`、`after-render.patch`、`after-test.patch` 后自行合入并重新决定 acceptance。

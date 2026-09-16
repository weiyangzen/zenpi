# ZS1-125 局部修复：正文查看器与审批预览不再裁掉 emoji 后的字符

实际 TUI 渲染反例：四个终端格宽的正文区域显示 `ab❤️X`，旧 wrap_segments 把心形基字符算一格、VS16算零格，将X误认为仍可放在同一行；Ratatui按两格心形绘制，X被裁掉。主控通过 TuiState 的真实 Alt-B 正文查看器和 present_approval + End 审批预览入口，在 TestBackend 终端buffer复现；前者窗口10×12，后者8×20，两者正文均四格。这里是实际 TUI 状态和绘制路径的内存终端后端，未运行真实 PTY、模型请求、工具写入或系统剪贴板。

新 tests/tui_preview_graphemes.rs 的三项测试在产品修复前全部失败（exit101）。两条可见画面均缺X，正文复制仍精确返回原文，草稿保持；审批请求仍在且Enter返回allow:false。这些前置断言先通过后再断言可见X失败，说明问题是显示裁剪。第三项明确检查heart、ZWJ家庭、地区旗帜按完整字素占格及正确行分配；旧版在第一组heart即失败，不把未执行到的另外两组称为before已复现。

修复仅将 src/render.rs 的 wrap_segments 改为使用已有 walk_wrapped_segments，按完整扩展字素、首字节样式和终端格宽发送分段及换行；换行时保留合并的Span，空输入和末尾newline仍产生相应行。聊天尾部原先已用同一walker。没有改TUI布局/几何、BentoBox、顶部加号目录选择、项目cwd、审批决定、复制载荷或持久化。旧 head代码块的wrap_plain、prefix scalar裁剪以及其他未复现公共API边界仍保留，不宣称所有Unicode和所有渲染入口已统一。

## 固定输入与结果

- 修复前 render：60098B/1706L，SHA256 b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d。
- 修复后 render：59562B/1688L，SHA256 9e526c42e1394cc2aa888f9627e7e0dfaf5b5e3acd0db3187c65553884d61c0a；只改变该函数，净−536B/18L。
- 正式前后同一新测试源码：3865B/104L，SHA256 964f5ab2853fd66234ca5d74a5f289298d3710183f4665e438c668fa0af84fad。before目录的初稿在rustfmt前保存；before-run.json绑定的是格式化后实际执行版本，后续测试未改。初稿与输入身份表原样留存，不把初稿hash冒充实际测试hash。
- TUI源码始终669221B/16376L，SHA256 801bffa879a211fc8b1fe3f43463dd38584f7f570b870d91f0c5ba2258d79c02。
- 修复后六组共85项Rust通过、0失败、0ignored：render内联23、tui_approval_focus12、tui_bentobox19、tui_preview_graphemes3、tui_transcript_ux17、view_model11。render另外137filtered，不计执行。新两个buffer画面中X独立换到下一行，完整心形保留；三类复杂emoji的精确行比较通过。
- cargo fmt --all -- --check、严格cargo clippy --locked --all-targets -- -D warnings、cargo build --locked --bin zenpi均exit0。全使用+stable-aarch64-apple-darwin。已有vendor/crossterm unused_parens警告保留，没有顺手改vendor。
- 新debug：50770984B，SHA256 63e40f29b01a82fd5b0d9f02069d10d466f68f3b707a370e27a44ba2c109714a。此二进制仅构建，未做新PTY/release/预算验收。不能继承旧380c896或80675c二进制的结果。

原始stdout/stderr保留前后TestBackend画面。root-before.py仅一次，原三失败保留；root-after.py各命令仅一次，整个运行前后123份src/tests/Cargo输入精确一致。环境仅设置私有ZENPI_HOME和CARGO_BUILD_JOBS=2，HOME/CODEX_HOME不变。命令argv、起止时间和exit在before-run.json与after-runs.json，日志hash在公开manifest。未测资源峰值；本局部修复不冒充完整G-CODE/G-HOST或预算通过。

## 独立阅读与剩余验收

主控本轮完整连续阅读修复前src/render.rs：1–300 bee85e、301–600 f71bb8、601–900 6503af、901–1200 6e7a9c、1201–1450 7ec500、1451–1706 896348，包含所有生产/测试和EOF；新的函数完整差分另读bccab5。当前096 worker报告只读1–100行857a68，基线901行和报告余文尚未独立完成，因此096仍未接受。保留其60098B冻结包；后续096接受需单独加入本次−536Bdelta，不重写worker历史输入。

相关宿主完整方法片段：正文查看器render935–1045（e06e5a）、审批render1210–1340（9132f8）；既有测试入口和copy/approval守卫按579810/28b52d/be5594/220540读取。仅此上下文，不宣称整个16376行TUI复核。

本局部显示修复没有引入job生命周期或持久化行为，取消和重启不新增状态迁移；沿用既有TUI回归检查其中已有边界。真实PTY、真实coordinator+tool执行、终端flags恢复和全阶段预算需要各自当前构建证据。117首启1039.683291ms>1000ms失败保持，未重试、预热或改门限。125既有审批计时/隐藏提示修复及其195Rust/14PTY是独立历史证据，保留[原记录](../master-approval-timer-3.1.21/review.md)，不合并计数。

Blueprint3.1.21 / run zenpi-stage1-20260911 / requirement3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d，53/121与snapshot3c01628b900c9078f7c1771f3638cc366b2e295c6fede7cfea191aa0620c5c2d保持。ZS1-124/125整项仍[ ]。正向/反向product.patch与rollback.patch只触及render.rs；回退保留用户会话、草稿与原始失败，本轮新测试可保留为失败规格或单独撤回。

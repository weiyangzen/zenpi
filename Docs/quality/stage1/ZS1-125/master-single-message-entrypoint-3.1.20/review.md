# ZS1-125 单条超长回复：最终验收入口接线

主库最终改动为 src/render.rs、src/tui.rs、tests/tui_transcript_ux.rs、tools/tui_transcript_ux_smoke.py，均属于蓝图ZS1-125列出的owned paths。新真实终端回归已并入既有脚本，以 `--single-message-only` 选择；早先临时独立脚本移除，精确原始执行字节仍保留在前一份冻结证据中。

功能与原始前后对照见[完整主控复核](../master-single-message-tail-3.1.20/review.md)：主库59项Rust、Clippy/fmt/debug构建通过；旧二进制真实PTY10/14，新二进制14/14；多消息历史5项与原有交互11项通过；独立C审查8项新增harness测试通过。该报告中“新增独立脚本”及其四文件补丁属于最初验证形态，由本记录的最终四文件清单替代，其余原始失败和证据保持不变。

本补充再次实际运行最终蓝图脚本入口：

```sh
python3 tools/tui_transcript_ux_smoke.py --single-message-only --binary .ops/stage1_execution/single-message-tail-integration/zenpi --evidence .ops/stage1_execution/single-message-tail-integration/entrypoint-final/actual-pty
```

退出0，两TUI、两loopback HTTP、14/14检查通过。这次重复是验收入口接线改变后的复验，不是新增14种覆盖，也没有用重试替代失败。新case函数与先前实跑版本逐字节相同（仅函数名改变）；原long_history函数不变、原main除新增选项/分发外逐字节相同，绑定见entrypoint-equivalence.json。最终四文件补丁在独立临时Git内正反check/apply及原始字节、无关哨兵检查通过，完整before/after均随包保留。

debug二进制SHA256 bdf901227bb2b20ae422f0802c3b361381952ba861b040320c401e6625f2cfa0，Rust源码从第一轮通过后未变。保留BentoBox与项目分页代码。本轮不声称整个ZS1-125完成：256KiB接受字节上限、宽度/Markdown重排的语义锚点、超大文档完整浏览及新release固定预算仍有边界或待验收。正式状态维持[ ]，只登记局部修复通过。

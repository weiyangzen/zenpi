
## 主控独立复核补充 — 3.1.20

主控已完成本源1286行全文、报告全文和19个精确有界上下文的独立阅读；验收只覆盖ZS1-023源文件理解。上文worker provisional、零产品运行及当时G-STAGE缺master的记录作为历史证据保留。当前接受凭据与实际门禁结果见 `Docs/quality/stage1/ZS1-023/master-review-3.1.20/`；不因此接受目标扩展能力、整项115或目录052。

C1 loader片段的退订循环没有catch：一个unsubscribe抛异常会阻止后续调用和末尾clear；第一次非空stale已保存，再次invalidate不会重试清理。该事实进一步限定上文“逐个unsubscribe再clear”为正常路径，不能解释为所有资源一定清理成功。source generic before的cancel短路、ToolCall直接异常传播、emitError监听器逃逸、共享对象mutation无回滚、默认cancelled:false无真实会话动作等资格均继续有效。只读测试断言与实际运行证据保持区分。

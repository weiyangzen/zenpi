# ZS1-132 / 126：跨项目新会话保留在途请求归属

修复一个真实JSONL错误：A项目的请求已被接收并排队，B项目`/new`随后完成；客户端在B项目重试A的请求ID时，最终响应可能被错误标为B，而该请求的provider事件和会话历史实际上仍属于A。

`ReplayState::reset_for_session`原先只迁移当前切换请求。其它在途请求的指纹和project绑定被丢弃。重试时第一层admit重新占用ID、写入当前B归属，第二层request_to_job虽然挡住重复执行，却已无法恢复原映射；没有重试时还可能以空指纹记录终态。本次迁移全部尚未终结的有限请求集合，切换请求优先、其余按稳定顺序写入新WAL，逐项保留原fingerprint与ProjectContext。已完成的旧session请求ID仍不迁移，不把旧会话缓存整包带进新会话。目标日志冲突仍拒绝，不覆盖旧live状态。

产品改动只在`src/headless.rs`，测试入口扩展原`tools/tui_session_new_smoke.py`，新增`--replay-only`与`--replay-cancel-only`。没有修改TUI、BentoBox、项目标题、真实cwd或授权规则。

## 原失败与修复验证

第一版before探针把原provider一直门控，却同步等待同一个BackgroundRunner后面的/new，导致fixture10s超时。该失败完整保留，但不用于宣称真实在途窗口已复现；headless runner串行排队与TUI独立new worker不同，不能沿用TUI后台新会话测试来推断headless调度。

随后before-queued预建两个项目，顺序接收A的首个门控请求、B/new、A的后续请求，再释放首个请求，并门控后续A请求。真实旧release `1d7b2e61ecc9096ad0f5e6cd3f3c03c6820cafe8371d9201de3497bb6b4e8f58` 返回2次HTTP，没有fixture超时；pending-a各事件标A，终态却标B，测试退出1。第二层仍返回duplicate_request_in_flight，因此不将此原失败误报为已发生重复provider执行。

最终native ARM debug二进制 `bb82450f8de71a0a7a91d828e42f659566cae3c7e4e574dd68706bfa10b94172` 上，两条真实JSONL路径共13项检查通过（正常6、取消7）：

- B/new完成后A排队请求仍在原session执行，重试相同或不同payload都不会改写在途指纹/归属。
- 正常终态及backend_cancelled终态都携带A完整project/cwd/session三元组；B不能取消A，切回A后才可取消。
- 完成后同ID同payload重放原终态，同ID不同payload报request_id_conflict；独立进程恢复到已提交的B新session transport后仍返回相同A结果，不增加HTTP。
- 每条路径2个真实JSONL进程、2次loopback HTTP，合计4进程/4HTTP。初次standalone版本通过后将同算法合入既有验证脚本，并用最终命令入口重跑；不把两轮重复通过累加。

## 回归与边界

79项Rust测试通过：新增ReplayState持久化/重启/目标冲突3项，headless_protocol43、project_workspace7、session_new13、headless_event_budget1、headless_project_workspace12。新单测覆盖原指纹不被不同payload替换、未终结请求重启为UnknownOutcome、不同payload冲突、旧completed ID不迁移、目标冲突保留旧live日志。

同一debug二进制上的既有/new三套实际入口20组通过：主流程8、policy9、recovery3（7TUI、11headless JSONL、31HTTP），包括原子新会话、CAS竞争、授权/草稿/背景项目、提交前后强杀恢复。严格库Clippy `-D warnings` 和headless rustfmt检查通过；原vendor crossterm警告保留。两个修改文件精确patch独立临时Git正逆apply/check及无关sentinel保护通过。

修复后的226个验证输入在构建之后、最终既有入口回归之前冻结，发布前逐项验证无漂移；不能把这份快照写成“构建前已冻结”。原始JSONL、HTTP、PTY、退出码和命令日志及前后源码均保留。此轮没有重建release或重复冷启动预算；上轮release8/8预算只是旧1d7b…二进制的证据，不覆盖此新改动。新release仍需最终验证，历史冷启动波动根因尚未定位。

恢复证明限于此连接实际切换到的目标transport日志；不声称实现了所有项目WAL的自动合并、任意旧路径重连或无限历史重放。已有单session transport日志范围、容量和截断合同继续存在。此增量也没有解决其它v2直写响应未带project、完整headless域事务或全部grace时序。

ZS1-132和126整项仍为`[ ]`，不豁免逐文件/目录依赖及其它交互验收。回退仅在检查后续修改后反向应用本包product.patch，恢复两个文件，不重置仓库、不删除用户项目、日志或会话。084候选冻结在旧headless源码，今后独立接受时须注明这份新修复，不能改写旧报告与历史证据。

# ZS1-071 — runtime.rs 3.1.20 完整复核候选

标准报告在 `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/runtime.rs_learn.md`。前12129B为原3.1.16完整报告，原文不改；追加本轮分别完整读取冻结基线763行、当前912行的结果及双runner接入约束。

- baseline-runtime.rs：28121B，SHA2183b622d96a8fdc084784af8f537fa09a4e1703b5ff70de3bc459d29fb05315。
- current-runtime.rs：33333B，SHA7a42c428083f8f3edb0019311296b479004ff562b548d7cdc504729f7cbd1f21，与旧实际执行subject相同。
- historical-target071-ready.tar.gz：原071整包，16payload+manifest；含139个输入的嵌套replay archive、33项历史实际测试日志及4项probe。原包完全不改，不重新执行。
- read-coverage.json / route.json：两份输入各自的连续阅读范围及authority/计数/范围。
- verify.py：纯离线校验旧payload、139归档输入、报告前缀、基线/当前关系及完整阅读范围；可选--packet在临时空目录做Docs-only补丁正反字节校验。

主控已收到JobId跨runner作用域、mark_completed与grace分支、阻塞事件背压和detach的具体锚点。其后主控说明计划隔离control状态、非阻塞poll并用独立结果slot识别真实完成。这是沟通状态；本项未读取或测试该新增host实现，不把说明作为验收证据。

从仓库根目录运行离线验证：

```sh
python3 Docs/quality/stage1/ZS1-071/worker-target-review-3.1.20/verify.py --root . --live
```

从ready目录验证其自包含payload与补丁：

```sh
python3 files/Docs/quality/stage1/ZS1-071/worker-target-review-3.1.20/verify.py --root files --packet .
```

--live只读取当前主库runtime文件并比对hash，漂移则拒绝沿用当前身份。归档源代码只是证据，不修改产品。新行为测试/Cargo/PTY/HTTP为0；33保持原快照、原测试scope。原包中的unpack和Cargo命令仅为历史可重放说明，本轮不调用。每个git校验命令限30s，无编译/子进程业务执行。

ready报告delta以主控报告不存在为base；worker提交是在已存在旧报告后追加，不能假定两者patch base相同。若接收处报告已经存在，先比较实际base，不强制套用new-file patch。主控独立G-FILE/G-STAGE验收，本候选不改authority或[x]。

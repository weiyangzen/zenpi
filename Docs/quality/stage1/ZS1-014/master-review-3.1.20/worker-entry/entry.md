# ZS1-014 主控逐文件复核入口补充

Worker A，`[_]`；主控验收独立。本轮只读源包、当前映射及已有本地依赖，新增测试/安装/构建均为零。原包的 authority 3.1.15 是历史记录，本补充不修改原包、主库、Blueprint、源报告或验收计数。主控表示 source012 已实际验证，012 结果不计入014。

## 冻结身份与完整读取入口

- 原包：`/Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/source014-ready`。
- 原 Docs 镜像：`Docs/quality/stage1/ZS1-014/worker-source`，提交 `5d01df6`。
- manifest：**`dd22f389f9bd98ded4766d34ce22251d78bc72a56de0c745de714064aca740eb`**，24 payload；本轮逐项校验全部字节/哈希及 Docs 镜像一致。
- 唯一验收 subject：`packages/agent/test/agent-loop.test.ts`，**1,610行 / 47,809字节**，SHA256 **`c359921a73525d67cf42481ad04210952fe36a9517d7b099f41710734438987d`**；upstream HEAD `bbb61e34aaf231639fdaaad1adbd757947034eac`。
- 完整阅读报告：原包 `learn-report.md`，SHA `cbf4e30b11e70f534e70b809469c297b16e548ce9ffda3d8556b8b3e6680cf85`。其已记录完整连续读取 **1–420、421–830、831–1230、1231–1610**，覆盖 fixture、每个断言和最后 continuation cases；字节范围 `[0,47809)`。本轮复用这份完整读取，不再制造一次全文读取/测试次数。
- 逐例入口：原 `case-review.json`，SHA `2ad7c50c18e35f8a55a8fa8c5e0661b5bf4244d08b3ac6c2476072adb22fc13b`。本补充 `case-table.md` 逐项呈现同一23行，不新增案例。
- 实测原始结果：`results.json` SHA `d4882a76f49b86f66b6192ac553a050ded0e966301014d73d62a8dfca98ac584`；`run.stdout.log` SHA `fdd5d8fe69540926b0dfed66e94fe334445dd682df77f4cc1871d21ab7276fd0`；stderr为空。校验实际 reporter 的23个 passed 标题与 case-review 一一匹配。

原冻结源共8份，全部与当前只读 upstream 同哈希。执行路径包含原测试、803行 `agent-loop.ts`、真实20行 `stream-fn.ts`、110行 EventStream、350行 validation；保留的 `index.ts`、原 `vitest.config.ts` 和 agent/package.json 提供导出/配置语境，其完整 package-barrel 初始化或原 monorepo 配置未执行。依赖文件支持本 subject，不自动增加文件/目录验收。

## 实际执行及 fixture 边界

历史实际运行：Node **24.19.0**，真实 Vitest **4.1.9** 与 TypeBox **1.3.27**，原测试文件零修改；**23 passed / 0 failed / 0 pending / 0 todo**。未使用计数stub或伪 expect，未增加补充案例；本轮不重跑。

`vitest.config.mjs` 仅选中这一个测试文件，`testTimeout:30000`、单worker。`pi-ai-runtime.mjs` 转发至冻结的真实 EventStream/AssistantMessageEventStream/validateToolArguments；`agent-index-runtime.mjs` 只转发原 `setDefaultStreamFn`。原测试直接导入原 `agent-loop.ts`；type-only imports 被转译器擦除。这里是隔离原测试运行，不是完整包 barrel、全仓 npm test 或 TypeScript 全仓类型检查。

原 fixture 原样保留：MockAssistantStream 继承真实 EventStream；queueMicrotask 发送构造的 done events；example.invalid 是模型元数据而无真实请求；createUsage 的费用为合成零值；消息带 Date.now；identityConverter 只筛标准角色。echo/edit/slow/fast 是实际执行的 JavaScript 回调和 Promise gate，**没有 OS 文件写入/子进程、真实 provider HTTP、持久化 session 或用户 UI**。测试标题中的 persist/source order 是内存消息/结果数组顺序，不是磁盘 journal。全局默认 stream 用 finally 恢复，没有被跳过的生命周期 hook。

不得扩大断言：870行 mixed slow/fast 只断言 slow 首启动且 fast 出现；1204/1439行终止案例各只有单工具；1031行 prepare snapshot 只断言替换 systemPrompt、prepare一次、stream两次。23例没有独立 global sequential overlap、长 prepare 两条消息到达去重、强制进程取消/reap、持久ID编辑撤回或崩溃恢复测试。更强的 source010/Zenpi 证据应单独引用，不改称014的断言。

## 当前 Zenpi 映射：保留、过时及边界

原报告将目标判据映射到 `stage1_input_queue` / `stage1_tool_batch` 的方向仍正确；`tests/runtime.rs` FIFO 作业队列不能替代同turn输入与工具批次合同。当前具体入口应使用下表，不能沿用“只需要 runtime/core_session”或“尚无实现”的早期表述。

| 原映射或结论 | 当前只读定位 | 本轮结论及证据界限 |
|---|---|---|
| 工具整批后收 steering/follow-up | `src/core.rs` complete_with_tools 约3609起使用 InputBoundaryGate；`tests/stage1_input_host.rs:328/332/337` 串行/并行边界、两lane all模式；101 scope 明确包含 input_queue 与 host 文件 | 已有专门 owner/host 实现和案例，不能继续描述为仅待搭建的 runtime FIFO。具体历史执行入口见 `.ops/acceptance101-sync-input-ready/index.md`；本轮不宣布重新通过。 |
| “长 prepare 新消息测试必须新增” | `tests/stage1_input_queue.rs:140/183` gate级空/非空快照；`tests/stage1_input_host.rs:800–915` 实际 Agent 准备阶段注入 late-a/late-b，one/all × 空/非空四例 | **“尚需新增”已过时。** Host用 CaptureOnly Backend、真实SessionStore，取消回调观察 `port.is_preparing` 再提交输入，检查每ID应用一次；不是这里新增的真实HTTP/PTy证据。原source014的1031断言本身仍不足以证明该合同。 |
| 串并行批次/全局或单工具屏障 | `src/core.rs:3986` 按全局标志、registry声明、扩展before/after hook与预算选择并发；调用 `tool_runtime::execute_tool_batch`；`tests/stage1_tool_batch.rs:184/237/265` 是实际HTTP owner用例 | 当前已经集成执行器，不能沿用旧“core串行/未接入”的描述。`.ops/acceptance102-index-ready/index.md` 已区分HTTP、真实文件/进程与helper边界；本轮不重跑也不新增验收。 |
| completion order 与 source-order result | 102真实HTTP案例184、executor案例424及后续 provider context/journal检查 | source014仅内存数组；102历史 reopen 是同进程，不能因名字自动升级为独立崩溃恢复。 |
| length工具零执行、参数/政策边界 | 102 current index 明确区分 provider终态解析拒绝与工具执行器的显式 truncated flag；当前批准preview/gate验证对应 tests/stage1_tool_batch.rs:591/629等 | 014的单工具length fixture、以及444行故意跳过hook后重校验的上游行为，不替代Zenpi实际wire或更严格批准边界。无需据上游差异修改Zenpi。 |
| Blueprint line77 “长 prepare 的消息快照测试” | source014实际681是整批工具后输入；1031只是准备快照接线；真正晚到/去重由上述Zenpi专门用例承担 | 作为映射导航可保留，不能把该文字扩大成014已执行长prepare到达竞争。主控authority原文不修改。 |

`mapping-identities.json` 记录读取时上述主库文件哈希，表示映射定位时的版本；未复制大源文件、未声称本轮完整重读所有大模块，也未给产品新缺陷结论。ZS1-058目录整合、101/102产品验收仍各自独立。

## 本地依赖、只读验证与精确重放

原 `runtime/package-lock.json` SHA **`f7652b5ff750c13dff743f0629d5a223259605b2f0dba1f00a14c8eeb71a2b1a`**。复用本地 `.ops/stage1-worker/source014/runtime/node_modules`，不安装、不联网。原014包只有lock/tree，没有逐安装文件历史清单：本轮生成的是**当前本地依赖**的紧凑整树摘要，不冒充旧运行时的逐字节后验库存。

当前依赖实测版本与锁定的 Vitest/TypeBox 一致；整树含 **2062 regular files + 5 symlinks**，规范排序的 `[相对路径,类型,大小/目标,内容哈希]` 编码摘要 **`e0c69c54e990df9fb5620e02f15cbdd2edf5301b7173941dc9deed953f3b0c10`**。只排除确切的生成结果缓存 `.vite/vitest/da39a3ee5e6b4b0d3255bfef95601890afd80709/results.json`。不用新增两千行文件清单，验证器会遍历每个实际文件并重算同一摘要。

`verify.py` 只读校验原24payload、8源码、23标题/状态及可选runtime摘要。Node固定字节 SHA `27db838bb204ef7c21df2931f5656e4c8fb32e6e947f363a402b49714d32b5b1` 已再核对。下面的校验已实际运行；不是测试运行：

```sh
python3 /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/source014-review-entry-ready/verify.py --packet /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/source014-ready --runtime /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/stage1-worker/source014/runtime/node_modules --expected /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/source014-review-entry-ready/integrity.json --upstream /Users/wangweiyang/GitHub/pi-mono
```

主控需要实际重放时用下列入口；destination必须是新的绝对路径。脚本复制原包和已有本地dependencies到新位置，执行原30秒timeout的 run.sh，再核对原包和原runtime未变。不会写旧Vitest缓存，不使用npm重新安装。

```sh
python3 /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/source014-review-entry-ready/replay.py --packet /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/source014-ready --runtime /Users/wangweiyang/.codex/worktrees/38de/zenpi/.ops/stage1-worker/source014/runtime/node_modules --node /Users/wangweiyang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/bin/node --destination /absolute/new/source014-master-replay
```

结果为新副本的 `replay-results.json` / `replay.stdout.log` / `replay.stderr.log`。原run.sh使用固定 replay 文件名，因此每次仍应使用新目录。本轮只检查新脚本语法及运行只读验证器，**没有执行此重放**。本补充仅供主控逐文件验收；撤回补充文档即为回滚。

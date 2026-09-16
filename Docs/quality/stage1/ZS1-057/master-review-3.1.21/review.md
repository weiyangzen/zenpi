## ZS1-057 主控独立目录验收 — 3.1.21

主控完整阅读 20238 字节候选（包含原始 3.1.18 报告），逐项核对物理 7 文件/2 目录及正式 010、011、054 三个直接子项。harness 下一层 12 文件/7 目录与 search 唯一 index.ts 均独立列清。非冻结的入口、proxy、类型与 search 合同仅为 context，不从此自动接受其它文件或父项 061。

本轮新完整阅读 index.ts 152 行、node.ts 2 行、stream-fn.ts 20 行、search/index.ts 27 行、types.ts 446 行、proxy.ts 402 行与 package.json 89 行，合计 1138 行。另读 agent-loop.ts 150–305 与 agent.ts 420–592 的实际衔接范围，核对队列先 drain 后 prepare、只在先前 pending 为空时补 poll、事件驱动公开 history、错误生命周期与 finally settlement。010/011 全文件理解通过本任务既有接受链复用；重新完整阅读二者原始主控语义 review 和 3.1.21 rebind，以及 054 主控 review。未把完整复制的其它文件或旧目标快照计为本次完整阅读。

目录集成理解：Agent 持有内存 transcript、队列及每次运行的 AbortController；loop 负责 provider turn 和工具批次，并通过 Agent.createLoopConfig 得到 drain、prepare、转换与终止回调。普通请求使用 transformContext→convertToLlm→key→stream。传入的准备回调可替换下一次 provider context，却不会自动修改公开 transcript 或提交持久 checkpoint。AgentHarness 是独立 durable session/lane/operation owner；同在 index 导出不意味着自动连接。054 已接受的普通 projection 与 raw summary preparation 差异继续保留，不能将取消 signal、浅复制或接口字段当成强制取消、深快照、回滚或恢复保证。

新增入口与类型复核：默认 StreamFn 初始未设置，getter 会抛配置错误；setter 可清空，package/dist 声明不证明已构建或浏览器兼容。search 只有接口，searchEntries 唯一可选方法，notify 同步返回 void；不是可用的检索 backend。类型文档中的 callback 不抛合同不能由 TypeScript 强制，prepare callback 抛错也不能类推其它 callback 的 safe fallback。after-tool 按字段替换而非深合并；全批 terminate 与 durable replay 字段的实际 owner 须区分。包名与旧 declaration merging 示例的差异按文档事实保留。

proxy 完整阅读确认：外层代理认证头与 payload options.headers 不同，signal 仅交 fetch/reader，options 序列化只选择声明字段。事件共享 mutable partial；text/thinking/tool delta 检查 block 类型，toolcall_end 错配却返回 undefined。逐行 `data: ` JSON 协议处理尾部 decoder 与无换行残片，clean EOF 缺 terminal 生成 error。未知事件默认警告而非 schema 拒绝，消费循环不会因首个 terminal 立即 break；catch 可继续修改同一个 partial，finally 仅移除 abort listener，没有显式 releaseLock。本轮是完整静态理解，不把后续共享对象变更、真实网络取消或恶意事件推断写成已执行反例。无完整代理服务器/认证/模型运行验证。

历史 057 测试已完整读取实际三用例、fixture helper、runner、bridge、Vitest/config，三个 actual observations、初次失败及最终 stdout/stderr，并逐字核对初末测试只有两处改变。processTool 虽在 helper 中，但本组只使用内存 gatedTool；不借其它项的真实子进程增加本组覆盖。模型流使用实际 EventStream 与 scripted responses；summary 是受控替身，checkpoint 是调用方人造对象，没有 journal 提交或完整 AgentHarness 实例。

D057-01 的四次请求依次接到 S1、S2、F1，准备 context 有 summary 和完整当前工具 turn、排除旧历史，公开 state 仍保留旧历史且没有 summary。D057-02 在 cooperative summary 取消后保留尚未 drain 的 late steering，新一轮同进程 prompt 使用新 signal 消费它；不是重启恢复。D057-03 明确复现 drain 后 prepare 抛错使 EARLY_STEER 同时离开队列且未进入 message_end/history；测试通过只证明这个丢输入反例，不能记为输入保全成功。

初次 2 pass/1 fail 因 keepRecentTokens=1 未产生预期切分，最终只改 fixture 为 8 并新增公开 state 无 summary 的断言，原排除 OLD_HISTORY 断言保留；真实结果 retainedTail 为 user/task、tool-call assistant、对应 toolResult。原失败和最终 3 pass 为历史证据，不是本轮运行，更不是性能或网络证据。

主控新复制冻结包并完整阅读只读 verify.py，从 /tmp 新运行一次，52 项离线事实全部通过。该检查核对 175 payload、历史 62 payload、子项 83 artifact 和 054 archive 157 payload；不执行捕获脚本。发布前另外逐项检查当前直接库存、源文件身份、三个正式 receipt 及其 artifact。上游身份相同用于历史结果复用；本次没有新上游/产品运行。目标映射限于既有 010/011/054 主控记录的范围，不声称当前大型目标文件或产品整项已完成。

仅接受 057 独立目录集成理解。当前 G-DIR 与 G-STAGE --item ZS1-057 由新发布脚本记录；蓝图要求 digest 保持不变。取消、输入保全、完整压缩、目标实现及父目录继续分别验收，历史失败不撤销。撤回只涉及本目录报告/receipt/状态，不改子项或产品。

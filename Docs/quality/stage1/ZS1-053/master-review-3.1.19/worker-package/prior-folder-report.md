# ZS1-053 — packages/coding-agent/src/core/tools

状态：[_] worker 独立目录集成候选，主控尚未接受各子项；不得据此提前标记依赖[x]。
source_path: packages/coding-agent/src/core/tools
run_id: zenpi-stage1-20260911
coverage_scope: frozen Stage 1 pi-mono subset, not the entire physical directory

## Immediate-child closure

本冻结子集本目录的直属文件恰为以下4项，没有被选入的直接子目录。真实目录中的renderers、truncate、path-utils等依赖仅context-only，不能借本目录报告声称其已完成。

| child | item/source_id | 完整字节范围 | SHA256 | 独立报告状态 |
|---|---|---|---|---|
| output-accumulator.ts | ZS1-024 / SRC-0953 | [0,6049) | c601ddd8e10934be6f3db30696eacba7b9544f2dd1648a5ec3c9935f0d4625c3 | [_] files/packages/coding-agent/src/core/tools/output-accumulator.ts_learn.md |
| bash.ts | ZS1-025 / SRC-0945 | [0,13665) | b76645f5d7b414957c7772eb8ec75d9dee71d27320ebcbf419881451576a6eee | [_] files/packages/coding-agent/src/core/tools/bash.ts_learn.md |
| grep.ts | ZS1-026 / SRC-0950 | [0,11550) | 88be3d00217d1a1caf8a9e7bb2b8d2e6d96edfa4b790c2cf76c746ab36b8841b | [_] files/packages/coding-agent/src/core/tools/grep.ts_learn.md |
| find.ts | ZS1-027 / SRC-0949 | [0,11178) | b06bcae6821a0e9fda1b63be613a7ce28eb0e66e1d01d16564e59e83f9ce10ea | [_] files/packages/coding-agent/src/core/tools/find.ts_learn.md |

四个文件均由本worker连续完整阅读并单独报告。主控接受目录前须核对当前manifest immediate_children正好这四项、各自read/report receipt/source hash和统一版本；此表中的候选状态不等于master接受。祖先core目录还依赖其它直属文件与extensions目录，不能由053递归代签。

## Cross-file integration

bash把stdout与stderr同送OutputAccumulator；append负责decoder/tail与spill，snapshot供执行中节流updates，finishOutput先拒绝late data/清timer再finish decoder和closeTempFile。temp path存在不代表完整写盘，二者都没有zenpi要求的quota/hash/complete/TTL合同。111候选因此在现有supervised命令读管道处分别捕获原始两流，owner在reap+join后验证终态，显示快照通过canonical view kind，host仍须完成progress/ref/read-action接线。

grep/find共享ensureTool、spawn/readline、路径格式化和truncateHead，但grep实际rg、find实际fd；两者customOperations边界不同。grep的context路径会整文件缓存，find的defaultOps.glob是占位，不能据这些辅助接口推定生产能力。zenpi112只使用已安装rg，构建ignore/glob独立交集、JSON context和NUL路径，保持原literal默认；不引入隐式下载或额外调度服务。

四文件的取消逻辑都需要与终态输出/子进程清理关联。zenpi保留更严格审批、immutable gate、生成artifact ID、call关联、bounded memory和single journal，而不是照搬宽松custom paths/exec hooks。工具并行资格属于102注册时显式声明；目录内容不能自动授权并行或worker文件访问。原始输出受111存储owner管理，搜索瞬态输出受112进程与pipe cap管理，两者职责分开且都由现有host预算/事件/恢复owner接受。

当前未完成条件：026/027/024/025和本目录需主控独立接受，111还需产品host/artifact动作，112还需core累计Processes预留和实际host回归。报告没有改任何blueprint checkbox、claim selector或master receipt。

# ZS1-026 — grep.ts

状态：[_] worker 完整阅读候选，主控未接受。
source_id: SRC-0950
source_path: packages/coding-agent/src/core/tools/grep.ts
source_hash: 88be3d00217d1a1caf8a9e7bb2b8d2e6d96edfa4b790c2cf76c746ab36b8841b
source_bytes: 11550
read_ranges: [[0, 11550]]
run_id: zenpi-stage1-20260911

完整阅读 1–323 行（1–280、281–323）。schema默认regex，literal显式true改fixed-strings；另有glob、ignoreCase、context和limit。这里与zenpi默认literal不同，移植不能把旧query的正则符号突然解释为regex。GrepOperations只替换isDirectory/readFile，默认执行依然实际spawn rg；ensureTool可能下载工具，zenpi本次只探测已有rg，不下载。

执行先Abort检查、幂等settle，再ensureTool和路径类型检查。rg参数--json/line-number/color=never/hidden，随后ignore-case/fixed-strings/glob与--pattern/searchPath。原文件没有显式ignore开关；rg正向--glob覆盖ignore规则（已核对本机rg15.2 help），因此description“respects .gitignore”有glob组合上的行为边界。zenpi候选使用ignore清单与独立glob清单交集，明确ignore=true不可由positive glob覆写。

stdout逐行JSON parse；忽略空行、解析失败、非match事件；limit按match事件计数，达到便kill child并记killedDueToLimit，source-order收集filePath/lineNumber/lineText。stderr字符串原实现无单独内存cap。abort设标志并kill子进程；cleanup关闭readline并撤监听。close时abort优先失败；除limit-kill外，code0/1成功，其余stderr或exitcode失败，不能把任意非零当成空成功。

无context直接用rg lines.text，规范CRLF/CR、删末尾换行并截长行。有context则getFileLines通过Map缓存整文件split行（未按源文件大小或总cache字节限制），逐匹配拼前后行，匹配行用path:line，其它用path-line；读取失败伪造unable-to-read-file行。最后truncateHead应用字节cap并附matchLimit、linesTruncated提示。zenpi候选直接消费rg context事件，避免二次整文件读取和无界cache，限制文件/深度/时间/pipe/行/结果/上下文并返回明确原因。

进程生命周期原来仅child.kill，不等价zenpi现有process group cleanup+wait。112复用SupervisedChild的清理/回收，取消不保留索引；tool参数以argv传入，禁用rg config、隔离子环境，默认literal路径仍用现有安全遍历。每次spawn必须计入host累计Processes预算：预检/清单/每批搜索均算启动，不能以最大同时1个进程替代累计预算。此文件是独立阅读候选，未用生产search测试替代逐文件阅读，也未提前接受112或其依赖。

## Independent controller whole-file review

Controller read all323 lines/11550 bytes and the complete worker report. Schema, default operations, definition factory, every execute branch, renderer spread and createGrepTool wrapper are covered. Imported renderers/ensureTool/path helpers/truncate/wrapper remain context-only; neither source code nor source tests were executed.

Precise differences: source defaults regex and hidden; target retains literal default and explicit hidden. Source directory paths are relative to searchPath and single-file results use basename; target returns workspace-relative paths. Source context reading normalizes CRLF/CR to newline, whereas direct rg text removes CR. The source close callback removes abort listeners before awaiting context formatting, so late cancellation during formatting is not guaranteed; _onUpdate is unused. An async close formatting rejection is not uniformly caught by the outer try; settle idempotence alone does not imply complete error containment. Source stderr/file cache lack separate byte caps; target bounds both traversal and output.

Target search.rs all566 lines and candidate tests453 lines were independently reviewed. tools.rs advanced options call actual installed rg. core.prepare_tool now persists cumulative Processes reservations before every search dispatch (literal0/find2/regex+glob maximum19); it does not treat an active slot as a reusable start budget. New real HTTP Agent/headless tests cover precise results in the continuation, insufficient budget with zero spawn, journal snapshot before first child, usage retained after reopen and cancel/reap. First cancellation fixture failed because a PID marker existed before its contents were written; trigger now requires a parsed PID, without extending sleeps or changing product cancellation.

Only026 file-understanding is accepted. Product112, directory053 and other source files still need independent gates. Current immutable Blueprint grants explicitly deny advanced search lacking confined traversal authorization; original literal search remains gate-aware.

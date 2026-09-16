# ZS1-089 — 主控逐文件理解验收 / 3.1.20

范围仅为 `.github/workflows/ci.yml` 的 G-FILE 理解闭合。主控接受完整报告及当前差量解释；不借此接受 ZS1-117、092、093，不声称整场远程 CI、新 release 或 artifact 上传通过。蓝图要求 digest：`8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`。

## 独立阅读与输入

主控已完整阅读冻结基线 77 行/2480 B、历史候选 94 行/3104 B、当前主库 113 行/3973 B；另完整读标准报告 227 行，连续范围 1–70、71–148、149–227，未用方法库存、diff 或 verifier 替代语义阅读。基线 SHA256 `5d7173fa20a0de949e53525018dad07718a9e4a9d24a5fcd137b056af31792d6`；当前 SHA256 `44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9`。报告 SHA256 `cad5882b88c6014a5d9d9f755e63cdcbf4a89b424801cbc0d52cc86fabcf0891`，原 24526 B 前缀精确保留，最终章节明确更正旧时点结论。

额外完整阅读 ready/verify.py、input-manifest、context-references、既有主控 CI 预算 review；check_modes.py 与 release.sh 为完整只读上下文；bench_runtime.py 仅 1–58、142–201、276–338 行，不作该兄弟 owner 完整接受。当前 verifier 再将三份上下文完整文件 hash 和所有摘录字节与主库逐一比较。蓝图的状态晋升会按设计改变其快照；此比较记录在晋升前。

## 全文件语义结论

触发器为无过滤 push/pull_request；contents:read，workflow/ref 并发取消；单 Ubuntu latest job，15 分钟。16 步顺序涵盖 checkout、稳定 Rust 和 rustfmt/clippy、缓存、fmt、all-targets/all-features Clippy、locked 串行测试、旧两版蓝图 checker、无 max 参数的 LOC 报告、预算与上传、两模式检查、fixture debug/release headless、user_smoke、生产打包。Clippy 未显式 locked；工具链/action 标签浮动，无 MSRV 或跨平台矩阵；没有部署步骤。旧 blueprint checker 命名不证明当前 Stage 1 checker 已接入 CI。

77→94 行仅生产归档检查改变：原 `! tar | grep` 可把 listing 失败混同无匹配；当前逐归档先要求 tar 成功，再拒绝禁止名称，grep=1 才代表未匹配，其余错误码失败；EXIT 清理临时清单。校验和步骤仍在之前。检查针对归档条目名称，不保证文件内容或类型无泄漏；历史坏归档案例不是本轮新执行，持续回归入口仍待补齐。

94→113 行仅预算和上传两步改变，前 9 步、后 5 步逐字节不变。预算通过 env 传 runner.temp/run_id/run_attempt 目录并全程引用路径，mkdir 不加 -p 且在 errexit 有效时拒绝旧目录。set +e 使管道失败后仍可保存紧随管道的 PIPESTATUS 数组；恢复 -e 后写 exit-code.txt，该文件只表示预算码。预算非零优先返回预算码；预算为零才返回 tee 码。写退出码文件自身失败可提前结束，不能保证任意 I/O 错误下保留预算码。samples 仍为 3，无放宽阈值、重试或预热。

预算脚本既有 startup 诊断为完整流长度/hash 与至多 64 KiB base64 前缀，非无限原流；summary 使用临时文件替换，早期失败可以缺 summary/部分样本。上传条件 always 且 outcome 非 skipped，上传整个同 run/attempt 目录，缺文件要求报错。目录创建失败但旧目录存在时，配置仍可能上传旧目录；因此拒绝覆盖不等于证明每个上传 artifact 都是本次新产物。runner 丢失、强制超时或取消可能阻止状态保存和上传。

主控在本次验收流程已只读查阅 GitHub 官方 [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)、[steps contexts](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts)、[status expressions](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions)：显式 Bash 使用 -eo pipefail；outcome 为 continue-on-error 前结果；含 always 的上传条件可在前序失败后评估。文档解释配置，不能替代 hosted Actions 实测。worker 本轮网络数为 0；主控文档查阅不应误记为网络数 0。

## 实际证据与限制

主控已运行新包离线 verifier，exit 0；344 artifact 和 337 payload 完整性、旧报告前缀、77/94/113 全覆盖、历史三包及日志/base64 绑定、两个接收基线的 8 次正逆补丁操作和 sentinel 均通过。主库标准报告此前不存在，只接收 absent 分支的完整报告，不叠加 append 补丁。

历史 worker 与主控分别执行同一五种 shell 场景，结果 0/1/17/23/17；本次只核验已封存原始结果，不计作十种案例或本轮五次新运行。tee=23 夹具先完成系统 tee 后返回 23，仅证明退出传播，不代表磁盘满或真实断管。旧目录拒绝/调用一次与原始字节证据保留。

本次新增当前检查实际解析三个 YAML 版本、逐步比较并核实完整上下文，且对每个当前 run 块执行 Bash -n 语法检查。它不执行任一 workflow run 字符串；无新 Cargo、PTY、HTTP、生产预算、发布或上传。完整离线 log 和 run 回执随本 review 发布，语义接受由主控完成，结构 checker 自身不被标作语义审查。

接受后只晋升 ZS1-089，保留尚未验证的远程 CI、当前 Stage 1 CI 接线、归档持续回归与平台矩阵差距。092/093 目录仍须各自独立阅读与主控接受。当前产品 CI 文件字节不变。

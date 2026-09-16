# ZS1-117 CI 预算失败证据保全增量（provisional）

本候选只修改 `.github/workflows/ci.yml` 中预算执行与其 artifact 上传两个步骤。仍为 `[_]`；不代表 ZS1-117、ZS1-089 或目录项验收。

预算步骤原本直接执行脚本，上传步骤没有状态函数，失败后会被默认成功条件跳过；原上传路径只包含汇总 JSON，没有脚本的启动诊断目录。现在使用 runner.temp 下由 run_id / run_attempt 区分的独立目录，记录 summary.json、budget.log、exit-code.txt 和 startup/sample-*.json。目录须新建，已有目录立即失败，不覆盖旧证据。startup 子目录交给预算脚本创建，遵守该脚本的“目录必须不存在”约束。

预算命令仍为 samples=3，没有 --no-fail、预算放宽或重试。stdout/stderr 合并后通过 tee 同时输出到日志。紧接管道读取 PIPESTATUS；预算非零时返回其原始退出码，预算成功而 tee 失败时仍失败。上传条件包含 always()，且预算步骤 skipped 时不执行；只上传本次独立目录，缺少所有文件时视为错误。artifact 名称也区分 run_attempt。其它 14 个步骤及预算前后文本逐字节保持。

## 精确输入与集成依赖

- 基线 ci.yml：3104 字节 / 94 行，SHA-256 `b9e70ba448d157341e0069fcd9ad19eae8f0412aaf0b9e03bbd97b4f2ef59dbd`。
- 候选 ci.yml：3973 字节 / 113 行，SHA-256 `44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9`。
- 单产品文件补丁 product.patch：1662 字节，SHA-256 `26f57a1ec753824b22aba4927cf4978560ef8a7b95e80f00380182905e846f59`。
- 对接主工作区现有 tools/bench_runtime.py：15327 字节 / 338 行，SHA-256 `ef6701f52659c78f58fb3f31adf87343c31b34229145f57acba2be05b1a9a52c`。读取预算常量、诊断写入函数及 main CLI（292–338 行）；完整精确快照附在 main-bench-runtime.py，采集时间见 baseline-state.json。
- 本 worker 的旧 bench_runtime.py 保持原样（SHA-256 `1d092022398742f6dc78a2d01cb84a4f93fb6dbcc8863e6eacf67bd466be9c66`）。候选需要集成到具备 --startup-evidence-dir 的主工作区版本，未声称旧 worker 脚本已经兼容。脚本本身不在补丁修改范围。
- 主工作区与本地 release.yml、继承的其它全部 144 个 tracked 文件、69 个既有顶层 ready 包的全部 10076 个普通文件（包含嵌套 manifest）均哈希核对未变。没有改 HOME/CODEX_HOME；主工作区只读。

## 本地验证

从候选 YAML 解析并实际执行预算 run 块，使用 bash --noprofile --norc -e -o pipefail 和临时 Python fixture。fixture 不运行产品、Cargo 或网络，只产生标记为 fixture 的日志和诊断。它严格检查转发参数、samples=3 及 startup 目录初始不存在。所有 5 个案例均使用含空格和中文的目录：

| 场景 | 预算退出码 | tee 退出码 | shell 实际退出码 | 保留内容 |
|---|---:|---:|---:|---|
| 成功 | 0 | 0 | 0 | 汇总、原始合并日志、启动诊断、退出码 |
| 超预算 | 1 | 0 | 1 | 同上，失败不变绿 |
| 汇总前失败 | 17 | 0 | 17 | 原始合并日志、部分启动诊断、退出码，无虚构汇总 |
| 日志器失败 | 0 | 23 | 23 | 明确使步骤失败 |
| 两者失败 | 17 | 23 | 17 | 保留预算原始失败码 |

每例再次使用同一证据目录时均由 mkdir 拒绝；真实 fixture 调用次数仍为 1，已有证据所有字节与无关 sentinel 未变。重复目录测试是验证拒绝覆盖，不是预算重试。日志 CRLF 字节与原 stdout/stderr 合并结果精确一致。fixture-results.json 保存各例真实退出码及全部 artifact 原始字节的 base64；fixture-bench.py 与 verify.py 可离线重放。

YAML 使用拒绝重复键的 BaseLoader 解析，保留 on 字段语义；完整结构比较仅两步变化。实际执行单文件补丁正向 --check / apply / 逆向 --check / apply，均退出 0，正向结果与候选精确一致，逆向还原基线，release sentinel 每步不变。

local-fixtures 命令退出 0，日志 SHA-256 `39a9062ef01e8415b2ed21f85852c6a5738c287828da820eb9542a3524b117ca`。真实 Ubuntu Actions、artifact 远程上传、生产预算、Cargo、PTY、HTTP fixture 和发布均未运行；本地测试不能证明 hosted runner 上传已成功。工作流取消或 runner 丢失仍可能使日志/退出码写入或上传不完整。

G-stage 主工作区只读检查真实退出 1：结构检查通过，缺少 ZS1-117.master.json。日志 SHA-256 `60eceb06118437035a2b02d9dc72b1fc3223dcba669fe1234f7cf58d4d6fb544`。该结果不是预算失败，也不用于将本候选标记为已验收。

## 官方语义依据

2026-09-12 查阅：[状态检查函数](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions) 说明没有状态函数时的默认成功条件与 always 行为；[上下文](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts) 提供 runner.temp 与 steps outcome；[upload-artifact v4 文档](https://github.com/actions/upload-artifact/blob/v4/README.md) 支持目录路径、缺文件错误和独立 artifact 名称。本次仅查阅文档，不执行 Actions 或产品 HTTP fixture。

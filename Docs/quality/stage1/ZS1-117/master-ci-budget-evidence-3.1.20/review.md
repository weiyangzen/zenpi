# CI 预算失败证据保全 — 主控整合，3.1.20

`.github/workflows/ci.yml` 的预算步骤现在将原始合并日志、预算退出码、汇总 JSON 和已有启动诊断写到本次 run/attempt 的独立目录；上传步骤在预算失败后仍可运行。过去预算失败会跳过默认成功条件下的上传，且上传范围仅有汇总 JSON，使早期启动失败的诊断无法保留。

主控完整读取 94 行当前 YAML、完整候选补丁及 120 行离线验证器，核对 worker manifest 的 19 个文件，并检查主库 `tools/bench_runtime.py` 的实际参数和诊断目录创建合同。补丁只改变预算运行和上传两个步骤，其余 14 步及前后文本保持原样。没有放宽 samples=3、阈值或加入重试；目录已存在时立即失败，避免覆盖旧记录。

主控从冻结候选 YAML 解析并执行实际 Bash run 块，使用五种明确标记为 fixture 的预算/日志器返回组合。实际退出码分别为 0、1、17、23、17；预算失败原码优先，预算成功而 tee 失败也保持失败。逐例确认原始 stdout/stderr 的 CRLF 字节、部分启动诊断、汇总有无、exit-code.txt 和调用次数。重复目录一律拒绝且不再次调用预算脚本，已有证据及无关 sentinel 字节不变。

验证器还执行单文件正向/逆向 apply-check 和实际应用、精确字节核对。主库随后独立执行 apply-check 和应用，确认候选摘要一致，`release.yml` 与 `bench_runtime.py` 保持原摘要。

- before：`b9e70ba448d157341e0069fcd9ad19eae8f0412aaf0b9e03bbd97b4f2ef59dbd`，3104 字节 / 94 行。
- after：`44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9`，3973 字节 / 113 行。
- patch：`26f57a1ec753824b22aba4927cf4978560ef8a7b95e80f00380182905e846f59`，1662 字节。
- 对接预算脚本：`ef6701f52659c78f58fb3f31adf87343c31b34229145f57acba2be05b1a9a52c`。
- `shell-cases.run.json` 记录原始命令、退出码 0、耗时与日志摘要；`shell-cases.log` 包含五例原始字节和正反向 patch 命令结果。

本次只证明工作流脚本和本地失败证据路径。没有运行 Ubuntu Actions、远程 artifact 上传、生产预算或 release；runner 被终止时也不能保证写入/上传完成。原扩展夹具失败、历史启动超时和当前源码的新 release 验证继续开放。ZS1-117、ZS1-089 及目录项没有因此被标记验收。

离线复验可在本目录的 `work` 副本执行 `python3 Docs/quality/stage1/ZS1-117/worker-ci-evidence320/verify.py --frozen`（依赖 PyYAML）；它只运行控制夹具，不执行产品预算。该副本 YAML 与已应用主库 YAML 摘要相同。

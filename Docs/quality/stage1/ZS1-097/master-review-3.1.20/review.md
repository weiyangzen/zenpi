# ZS1-097 Cargo.toml 单文件主控接收（3.1.20）

接受范围仅为根 `Cargo.toml` 的完整声明、基线差异、必要调用映射和已知边界学习。主控完整读取基线 42 行 / 1001B、当前 50 行 / 1228B，完整阅读 24001B 候选报告（按 1–47、48–91、92–150 连续阅读）、两个离线检查器、17 个选定 lock 块，以及 vendor feature/reset、core fixture admission、skills YAML/ignore、TUI grapheme/editor、发布与预算脚本的必要片段。没有把上下文片段计为其它文件或目录的完整学习。

基线 SHA256 `94c25405859cbec9d1a02e7c1a0a4e0dbbfd96e5c6bc476c23b9dc8be7229a65`；当前 `d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884`。本文件没有源函数或内联测试，不通过虚构行为实验来填写声明文件的覆盖数。

## 独立结论

1. 全部语义增量只有 ignore、unicode-segmentation、yaml_serde 三项普通依赖、release strip=symbols 及 crossterm 本地 patch；其余 TOML 语义与冻结基线一致。使用标准库 tomllib 实际解析两份完整源验证该结论，而非只依赖文本搜索。
2. 本包 default=[] 与 dev-fixtures=[] 保持独立；依赖的默认 feature 仍按各声明处理。ratatui 关闭自己的默认 features，crossterm 保留默认集合及 bracketed-paste。release profile 不等于生产 fixture 准入策略，panic=abort 也不能证明终端在所有 panic 路径恢复。
3. 本轮实际执行 native `cargo metadata --format-version 1 --no-deps --offline --locked`，核对根包 edition=2024、声明 rust-version=1.88、features、16 条依赖（15 normal 含 cfg(unix) libc，另1 dev tempfile）、精确 unicode-segmentation 要求、ratatui/crossterm 默认 feature 字段及真实 example 路径。`--no-deps` 不生成 resolve 图；没有把这个命令说成完整传递 feature 图或 MSRV/全平台构建验证。
4. 本地 crossterm 同名同版不能单凭版本或 purl 区分源字节；reset API 的 Unix/非 event-stream 条件和当前外部编辑器调用耦合明确。版本范围、lock 所选版本、本地源身份、平台实际编译集合分别解释，预算的15项是声明名称口径。
5. 新 YAML/ignore/grapheme 依赖有实际调用连接；完整资源、输入、编辑器或渲染正确性仍归其独立产品项。Cargo.lock 的17块只作上下文，没有据此接收098或vendor逐文件库存。
6. 候选历史 reader-reset helper 和1d7 release证据只作已冻结来源；原失败记录与平台局限保持。当前源码已发生 headless/TUI/attachment 等后续改动，历史 release 预算不能覆盖当前版本。没有重新执行这些历史产品实验，也没有把全库96项通过算作本声明文件的源测试。

## 接收时的上下文差异

对候选的十个上下文对象再次捕获当前全文件摘要，并逐片段定位其原始字节。除 CI 的长片段以外，其余选定片段仍逐字节存在；TUI 全文件已改变，相关 grapheme/editor 片段仍保留。CI 上一轮新增预算失败日志/诊断上传，当前113行版本已完整审阅，原 clippy/test/fixture/production 打包命令保持。具体当前行号、旧/新摘要及存在性记录在 `semantic-current.log`；这份增量说明覆盖候选冻结时刻之后的差异，原报告保持原字节。

## 验证和回退

离线 verifier 独立检查完整78个候选artifact、60份捕获输入、历史收据子集、两个接收基线均 absent；在临时 Git 中分别执行 master/worker 补丁的正向检查/应用、逆向检查/应用，精确匹配报告并回到不存在状态，无关 sentinel 不变。实际运行日志、命令、退出码和摘要随本接收记录保存。完整原 ready 压缩包保留，可解包到新目录后运行其中 `Docs/quality/stage1/ZS1-097/worker-cargo-manifest-review-3.1.20/verify_package.py`；该 verifier 以 ready 根目录为上下文，不应直接从主库扁平复制出的支持文件路径运行。

接受条件为本文件内容已被完整、准确地理解和映射，未要求把报告列出的所有跨平台产品实验归到097完成。Rust1.88实测、全 feature/target 矩阵、当前 release 预算、其它 owner 和目录继续保留各自未完成状态。主控接收不改变这些结论。

回退只撤本项标准报告、此次证据/receipt和本项状态，恢复备份的索引/selector/todo；不改 Cargo.toml/Cargo.lock、其它产品文件或用户会话。

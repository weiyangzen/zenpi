# ZS1-097 — Cargo.toml 单文件完整复核

状态：worker candidate；master G-FILE / G-STAGE 待独立接收。模式 understand；产品改动0。本项只生成标准路径的一份学习报告与新证据；不修改主库、蓝图、索引、claim 或完成标记。

## 权威、基线与全文阅读

蓝图 `Docs/stage_1_v3_pi_mono_blueprint.md` 3.1.20 第122行将 ZS1-097 绑定至 Cargo.toml：1001B，SHA256 `94c25405859cbec9d1a02e7c1a0a4e0dbbfd96e5c6bc476c23b9dc8be7229a65`。第211行 owned path 为 `Docs/learn/stage1_pi_mono/targets/zenpi/files/Cargo.toml_learn.md`，依赖 ZS1-001，验收 G-FILE/G-STAGE，回退仅撤本报告。第236行根目录 ZS1-091 依赖本项及098；第514行131另负责依赖 patch/vendor/helper。097不接收098 Cargo.lock、089 CI、131 vendor、113资源、117预算或其它产品 owner。

冻结 selector 的 run_id 为 zenpi-stage1-20260911，active=true；requirement digest 为 `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`。完整蓝图与 selector 原字节保存在 context；snapshot 是状态快照身份，不能与稳定 requirement digest 混用。机器核验会检查蓝图字节 hash 与 selector 的 snapshot_sha256 以及本项基线身份。

基线从 git revision `6f252a20c628e9b1ede14e2887acc04657c71d7c:Cargo.toml` 提取，只有因其1001B及hash精确匹配权威基线才采用。不是用 HEAD 代替用户修改后的基线。当前同时只读核对主库与 worker 的 Cargo.toml，字节相同：1228B/50行，SHA256 `d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884`。基线42行与当前50行均从首行到末行连续全文阅读，随后对冻结副本完整复读；较大混合输出的截断不计替代完整阅读。read-log记录两个身份与覆盖范围。

正式源文件数1；两副本不重复计数。它是声明文件，Rust函数数0、源内测试定义/阅读/实际执行0。本轮新增 Cargo/PTY/HTTP/产品行为执行全部0；只执行文件/hash/JSON/限定文本提取和隔离 Git patch 的证据机械验证。没有将正则当成 TOML 解析器或 Cargo resolver。曾尝试导入系统 Python tomllib，因 Python 3.9 未提供而失败；未运行产品或安装依赖，改用明确受限的文本提取。

接收前 worker 与 master 的标准报告路径均不存在；receiving-baselines.json记录 absent，不伪造旧空文件或旧报告。既有所有 ready/旧候选保留，source-drift.json保存并核验先前冻结 manifest 身份。目录报告仍须由主控接受的文件报告派生，本项不自行推进目录或整阶段完成。

## 基线到当前的全部变化

新增3项普通依赖：ignore="0.4"、unicode-segmentation="=1.13.3"、yaml_serde="0.10"。release profile 新增 strip="symbols"。新增 ZS1-131 注释与 [patch.crates-io]，将 crossterm 指向相对路径 vendor/crossterm。其余原有包字段、features、依赖要求、Unix libc、tempfile、example 和 release 键值保留。原始精确差异在 source/baseline-to-current.diff，只作解释证据，绝不是供接收的产品补丁。

基线普通无条件依赖11项，当前14项；加入 Unix 条件 libc 后声明 normal 名称集合由12变15。tempfile另属dev，共16条直接依赖声明。增量227B不等于227行或产品代码行数；本文件从42行变50行，8个新增物理行包含注释/表头/空行，不能冒充功能实现LOC。

## package 与 feature 逐项解释

name=zenpi、version=0.1.0标识包；edition=2024选择语言 edition，rust-version=1.88声明最低 Rust。description 描述 TUI/headless JSONL，license=MIT、repository、readme、5项 keywords 是发布元数据：没有在这里实现双模式、下载仓库、读取用户文件、验证许可证兼容性或保证文档存在。版本0.1.0不表示依赖版本都固定，Rust1.88也不是对当前完整依赖图已实测的证明。

[features] 只有 default=[] 和 dev-fixtures=[]，均无依赖激活列表；所有 normal 依赖均未标 optional。default=[]表示本包不默认开启自有非默认feature，绝不关闭各依赖默认features。dev-fixtures虽是空列表，仍能被 Rust cfg 使用；core 5727–5738片段要求 backend_explicit 且 cfg!(feature="dev-fixtures") 才允许 echo，否则返回fixture错误。cfg!是布尔常量分支，不应据此宣称 EchoBackend 源码完全被条件编译排除或二进制从无相关字符串。

默认、显式 --features dev-fixtures、--all-features 必须分别看待。release只是优化profile，不自动关闭fixture；CI的 release smoke明确启用fixture，生产打包脚本的 build没有传features。也不能从终端事件中的cfg feature误推本包 --all-features 会自动打开所有依赖feature；本包这里只有default/dev-fixtures，而依赖图其它请求仍可能合并features。这里只核对已读声明和必要边，不虚构完整激活树。

## 全部直接依赖、当前锁定版本与边界

下表版本来自当前 Cargo.lock 的17个必要 package块之一；这些块分别带原文件hash、捕获时刻、行号与切片hash。只读这些块，不能称098完整lock学习已完成。用途未读取调用者时仅表述依赖职责，不推导实际调用覆盖。

| 依赖 | 当前声明（原行） | lock版本 | 静态角色/feature边界 |
| --- | --- | --- | --- |
| base64 | `base64 = "0.23"`（L17） | 0.23.1 | 二进制文本编码依赖；本轮未展开调用者 |
| crossterm | `crossterm = { version = "0.29", features = ["bracketed-paste"] }`（L18） | 0.29.0 | 终端事件/控制；显式 bracketed-paste + 默认 features；本地 patch |
| httpdate | `httpdate = "1.0"`（L19） | 1.0.3 | HTTP 日期解析/格式化依赖；本轮未展开调用者 |
| ignore | `ignore = "0.4"`（L20） | 0.4.33 | 新增；skills 片段构造 GitignoreBuilder，读取 .gitignore/.ignore/.fdignore |
| ratatui | `ratatui = { version = "0.30", default-features = false, features = ["crossterm"] }`（L21） | 0.30.2 | TUI；本依赖 default-features=false，仅显式 crossterm backend |
| serde | `serde = { version = "1.0", features = ["derive"] }`（L22） | 1.0.229 | 序列化框架；显式 derive，默认 features 仍开启 |
| serde_json | `serde_json = "1.0"`（L23） | 1.0.151 | JSON；默认 features；不由此证明 JSONL 模式完整 |
| sha2 | `sha2 = "0.10"`（L24） | 0.10.9 | 哈希依赖；不能仅从声明推导安全用途和覆盖 |
| thiserror | `thiserror = "2.0"`（L25） | 2.0.20 | 错误类型派生依赖；不意味着所有错误恢复 |
| toml | `toml = "0.9"`（L26） | 0.9.12+spec-1.1.0 | 配置解析依赖；本轮不接收配置 owner |
| unicode-width | `unicode-width = "0.2"`（L27） | 0.2.2 | 显示列宽依赖；与 grapheme 边界职责不同 |
| unicode-segmentation | `unicode-segmentation = "=1.13.3"`（L28） | 1.13.3 | 新增；精确版本约束；TUI after_edit 用 grapheme_indices(true) |
| ureq | `ureq = { version = "3.4", default-features = true, features = ["json"] }`（L29） | 3.4.0 | HTTP；显式 default-features=true + json；lock 记录含 rustls 等 |
| yaml_serde | `yaml_serde = "0.10"`（L30） | 0.10.7 | 新增；skills frontmatter 的 Value/from_str；解析错误映射 SkillError |
| libc | `libc = "0.2"`（L32） | 0.2.189 | 仅 cfg(unix) 的直接 normal 声明 |
| tempfile | `tempfile = "3.23"`（L35） | 3.27.0 | dev-dependency；不计 normal 预算 |

没有等号前缀的版本串是兼容范围，不是精确版本锁；当前唯一显式等号精确要求是 unicode-segmentation =1.13.3。Cargo.lock提供本次选定包身份，未来更换lock时其它范围可重新解析，--locked约束不能替代源身份、平台与feature证据。锁定版本和 manifest 要求须分列，不能把 tempfile声明3.23写成当前解析恰3.23，也不能把crossterm0.29当成一次升级。

ratatui显式关闭自身默认features并选crossterm；这不自动关闭 crossterm 自身默认features。当前 vendor manifest 默认集合为 bracketed-paste/events/windows/derive-more；events接mio/signal-hook/signal-hook-mio，derive-more接可选derive_more，windows接winapi/crossterm_winapi，是否按target编译仍看target条件。root显式bracketed-paste重复列在默认集合中，不是单独最小终端feature集合。root未请求event-stream/osc52/use-dev-tty；本轮没有遍历整个依赖图以排除其它feature请求。

serde启用derive；serde_json、toml、yaml_serde没有关闭默认features。ureq显式 default-features=true + json，选定lock边含rustls、rustls-pki-types、webpki-roots、flate2、cookie_store等，不可宣称只含JSON或网络/TLS功能被裁掉。lock依赖边不是逐feature激活证据。base64和哈希库出现在manifest不代表协议编码/签名/凭据安全已经验证。

[target.'cfg(unix)'.dependencies] 的 libc 仅直接用于Unix条件；这没有在当前文件实现 Windows fallback，也不能据此说Windows图中永远没有传递libc。tempfile是dev-dependency，可用于测试/示例开发情境，不能按root lock的16条边全部计作正常生产依赖。root锁块是解析记录的依赖联合信息，不能用它反推出单target编译图。

预算脚本 dependency_receipt 使用 cargo metadata --no-deps --locked，将根包 dependencies 中 kind 为 None/normal 的名称集合排序计数，没有按target表达式过滤，再另收 cargo tree --depth 1 --locked。因此声明normal集合15与Unix当前历史树15一致，但不能将预算 normal_count 泛化为Windows实际编译数。预算上限16只剩1项声明名额；传递依赖规模、平台差异、维护成本和二进制体积均不由这个数字独立证明。脚本这里只读，不执行其Cargo命令。

## 本地patch、平台条件与调用耦合

[patch.crates-io] crossterm={path="vendor/crossterm"} 在根包解析时提供本地替代来源，路径相对manifest目录。vendor package仍名crossterm、version0.29.0、edition2021、rust-version1.63.0；局部较低MSRV不代表整个Zenpi图的MSRV。当前lock中crossterm0.29.0没有registry source/checksum，与本地patch一致。版本字符串不再足以辨认registry原版与修改版，需要vendor库存/文件hash。--locked不锁住本地源字节，也不保证离线依赖缓存完整；离开包含vendor的源目录或向其它消费者发布时，必须单独证明patch/源码分发边界，不能从本地构建成功推导。

vendor event.rs 145–166显示单例 INTERNAL_EVENT_READER，reset_event_reader通过try_lock取锁，锁忙返回WouldBlock，成功take并drop reader；下一poll/read重建。注释明确不清空OS输入队列，调用时须停止所有事件reader及cursor query。API条件为 all(unix, not(feature="event-stream"))。Unix调用片段将它传给guard.suspend_editor/resume_editor；非Unix分支提示不可用。若未来在Unix图启用event-stream，该API会被移除，当前这些调用仍需联动处理；这是静态集成约束，本轮未执行构建复现。

本项只学习根manifest的选择与必要调用边。vendor文件库存hash相等不等于77个第三方文件逐一语义复核；WouldBlock helper通过不等于130编辑器产品全部时序成立。回退产品patch必须同时核对131的Cargo/lock/vendor与130对reset API的依赖，不得在保留调用时独立恢复registry原版。本报告交付的补丁仅新增报告，不执行该产品回退。

## 新增依赖与实现连接的最小证据

skills 601–638的 yaml_string 区分 Value::String 和其它类型；parse_markdown使用yaml_serde::from_str并把解析错误映射SkillError，片段也显示frontmatter标记和大小检查的一部分。710–730构造GitignoreBuilder，添加.gitignore/.ignore/.fdignore逐行规则。足以确认新依赖有实际代码连接，尚不足以证明完整资源发现优先级、嵌套忽略语义、取消/错误清理或全部YAML边界。

TUI 1284–1300的 PasteMetadata::after_edit 用 grapheme_indices(true)生成字节索引并追加input.len。unicode-segmentation与unicode-width分别支持分段边界/显示宽度，两者不能互代；精确固定版本有助于保持分段规则输入身份，却不能独自证明所有游标、折叠、粘贴、emoji/ZWJ交互正确。085/122负责完整TUI/composer行为，本项不接收它们。TUI是捕获时刻的一次副本，后续主控改动不被本报告默认为已读。

## example、profile 与实际发布路线

[[example]]显式命名 runtime_budget_probe，路径tools/runtime_budget_probe.rs；没有required-features字段。根manifest没有额外声明lib/bin/bench/workspace/build-dependencies，不代表Cargo自动发现目标不存在。预算脚本 run_probe 对此example使用 cargo run --release --quiet --example runtime_budget_probe --locked，在Darwin ARM插入指定stable toolchain。例程运行与产品启动是两种进程证据；本轮没有阅读整个probe源码，更没有执行它。

release opt-level="z"偏向大小，lto="thin"与codegen-units=1改变优化/编译策略，strip="symbols"裁剪符号，panic="abort"选择异常panic时终止而非unwind。它们不能保证体积、启动耗时、RSS或队列/渲染阈值，也不能保证栈追踪可用。abort不承诺panic时运行栈上清理；正常Result错误、显式取消与关闭路径仍有不同语义，不能把panic配置泛化为所有错误都不可恢复或所有终端状态都能恢复。

CI定义ubuntu-latest + stable，没有在已读片段出现Rust1.88最小版本矩阵。clippy --all-targets --all-features未带--locked；test同组合带--locked且串行。headless/debug/release smoke及user_smoke显式使用dev-fixtures；这类通过不能独立证明默认生产fixture门禁。最后tools/release.sh使用--release --locked --target并未传features，重新构建后拷贝该target路径二进制；是否最终有效仍须绑定实际argv/toolchain/features/hash，不能从先前fixture smoke推导。

release workflow声明5目标：macOS x86_64/aarch64、Linux GNU x86_64/aarch64、Windows MSVC x86_64。Unix调用release.sh，Windows直接--release --locked --target。定义矩阵不是各平台已跑收据。本轮没有平台构建通过数。release.sh可由ZENPI_RUST_TOOLCHAIN选工具链或按host找已有toolchain，不等于精确固定1.88。

打包的SBOM来自 cargo metadata --locked列出的源包name/version/purl，没有以二进制实际链接可达性或target过滤证明组件列表。对local patched crossterm，只有同名同版purl也不足以表达自定义源hash；已有独立vendor库存能补充身份，不能把SBOM本身说成已携带该证明。脚本的删除stage/归档命令仅作为文本阅读，从未运行。

## 历史真实运行与本轮零执行分账

历史ux131候选在3.1.19权威下独立运行，不能转写为本轮3.1.20新运行。冻结其README、manifest、dependency-experiment-manifest、inventory、result、3个Cargo日志及10个PTY原始文本。result记录Darwin ARM，before/after两次独立build与vendor内部测试exit0；before build未带--locked，after与内部测试带--locked。每条argv/cwd/start/end/log hash保留并可与副本校验。before helper二进制hash为 `f58aea54622e51642d91e6dde2808ee824b01e00f8707a897f2817f99fc9a1e7`，after为 `b305ee5cf2b043d118042c599cf1d85ccb831f5f007c88e9b81419208fe30092`，不冒充产品release。

4组before/after的相同是输入字节：queued、paste、CSI、UTF8。queued两侧先drain Enter后都收到x；paste原版拼成BEFOREAFTER，reset后收到A；CSI原版Up/reset后A；UTF8输入两侧都有尾随x，原版先组成界/reset后读x。另有after-only queued-CR reset，以及100次cycle记录FD base/max/final=7、SIGTERM/SIGWINCH各100。10条PTY记录不是10个全产品场景；没有Linux实际运行。历史README保留最初不能测试非workspace依赖、离线dev依赖缺失、以及早期UTF8输入不一致的失败/修正说明。本包引用该历史manifest，不声称重新运行或已独立读取它引用的每个旧失败原日志。

后续冻结release历史来自094 ready中的release副本：二进制 `1d7b2e61ecc9096ad0f5e6cd3f3c03c6820cafe8371d9201de3497bb6b4e8f58`，6033168B，Darwin ARM；build-inputs的Cargo.toml与Cargo.lock身份与本轮捕获相符。预算历史8/8通过，normal15，5次启动最大751.967292ms，运行进程峰值RSS5111808B，queue1593.783µs、render662.788µs、layout1.07245µs、10000dirty合1帧。阈值保持16依赖/8MiB/1000ms/96MiB/2000µs/10000µs/100µs/1帧。

该历史仅证明冻结release的一组样本；不证明strip单独导致改善或启动波动根因已解决。review保留旧1044.823959ms及更早1208.732708ms失败的指向，旧证据未覆盖。本包并未复制review所有相对链接所指文件，离线验证不把这些外部链接当成存在的本包artifact。历史budget.run的1.95GB是编译/工具进程树，不是Zenpi运行时RSS；包装器PID不是已确认产品PID。历史review仍complete=false，不能据此将整个117/131或蓝图勾选。本轮只核验这些收据的字节与有限内容，不累计它们为新测试。

## 能力组与尚未执行的最小实验

下面12组用于明确证据缺口，均非本轮产品实验结果。第12组中的文件补丁/离线检查属于本轮机械验证，其余所列Cargo/进程行为均未执行。

| 组 | 能力 | 最小实验/输入 | 当前边界 |
| --- | --- | --- | --- |
| 1 package-msrv | 包身份、2024 edition、Rust 1.88 声明 | 隔离的 Rust 1.88 与 stable、锁定同一输入构建；记录 toolchain/target/feature/exit | 声明与 CI stable 已读；最低支持图尚无本轮运行 |
| 2 root-features | default=[] 与 dev-fixtures=[] | 默认/显式 fixture/--all-features 三组，分别检查显式 echo 与普通 backend 入口 | core cfg! 准入片段；CI 的 all-features 不代替默认生产 |
| 3 dependency-defaults | 依赖默认 features 与图合并 | 同一 lock 的 feature tree，特别关注 ureq TLS、crossterm events/event-stream、ratatui 后端 | 声明/选定 lock 边；未求出完整激活图 |
| 4 dependency-count | 14 无条件 + Unix libc + dev tempfile | 固定预算脚本，比较 metadata normal 集合与各 target 实际树；不得把 dev 算 normal | 当前声明与历史预算 normal_count=15；不是每平台编译数 |
| 5 patch-provenance | 本地 crossterm0.29.0 patch 身份 | 独立原包与固定 vendor 全库存差异检查，绑定 lock/source/API caller 版本 | 只复用历史库存和片段；本轮未全文复核 vendor |
| 6 reset-availability | Unix 且非 event-stream API 条件 | 有/无 event-stream 及非 Unix 构建矩阵；并发 reader WouldBlock 和 OS queue 边界 | 条件直接可见；历史 Darwin helper；Linux/Windows 本轮0 |
| 7 ignore-yaml | 新解析/忽略依赖连接 | 合法/畸形 YAML、严格字符串、ignore 否定/继承/失败、取消；保持调用owner独立 | skills 片段只确认使用；113/078不由此接收 |
| 8 grapheme | 精确 Unicode 依赖与字节边界 | 组合音符/ZWJ/emoji/宽字符编辑，检查光标/折叠/删除一致 | after_edit 使用边界；122/085完整行为仍独立 |
| 9 release-profile | z/thin/strip/codegen1/abort | 同一输入 production release 体积、首次启动最大值、RSS和队列/渲染预算 | 历史1d7二进制通过；本轮未重新构建或归因 |
| 10 example-dev | runtime_budget_probe example 与 dev 范围 | 构建/运行指定 example，独立绑定探针与产品二进制，不混淆 runtime/编译器RSS | 仅声明与脚本调用片段；117仍独立 |
| 11 release-matrix | CI fixture smoke 与生产打包边界 | 每一目标打包后记录真实 features/二进制/hash/冷启动/最小help和fixture拒绝 | 五目标只是 workflow 定义；无本轮平台通过数 |
| 12 rollback-evidence | 报告接收、补丁正逆与历史完整性 | absent 基线隔离 git init，forward/reverse check/apply，逐字节/hash/sentinel，再离线验证 | 本轮只执行证据机械检查；产品回退由131/130协调 |

## 必要上下文的时间与全文件身份

以下10个文件仅按列出的冻结切片解释。完整文件hash是切片来源身份，不是阅读完成率；Cargo.lock只抽17个package块，vendor/CI/tools/callers都不计正式owner覆盖。每段原字节与行号见context-capture.json，完整inputs.json还列权威副本与历史副本身份。

| 文件 | 捕获UTC | 全文件字节 | 全文件SHA256 | 冻结行段 |
| --- | --- | --- | --- | --- |
| vendor/crossterm/Cargo.toml | 2026-09-12T11:38:48.489636+00:00 | 4455 | `78b5e8b5cd6660067195979533e61d4b33c375c74613f24180efd94c86f7d295` | 12–40, 48–88 |
| vendor/crossterm/src/event.rs | 2026-09-12T11:38:48.489853+00:00 | 63629 | `989c472a11a6b471e663e191fd64337593423de2aa4774344cb246fb975bbd42` | 145–166 |
| src/core.rs | 2026-09-12T11:38:48.490083+00:00 | 278737 | `52311b129c0bb467abac36de216f5bc97bf3c4c42d03dcd6170a717339c2a869` | 5727–5738 |
| src/skills.rs | 2026-09-12T11:38:48.490656+00:00 | 40533 | `a6d0a2f671febdef2e631ca53d2788ab869ab7b86db6c0cb1feaf24d128f83e8` | 601–638, 710–730 |
| src/tui.rs | 2026-09-12T11:38:48.490967+00:00 | 587952 | `19fcdd4d7cb98e45096bfb122300815738dd80734d671c7a7714002f6af4d339` | 1284–1300, 12688–12748, 12755–12772 |
| .github/workflows/ci.yml | 2026-09-12T11:38:48.492134+00:00 | 3104 | `b9e70ba448d157341e0069fcd9ad19eae8f0412aaf0b9e03bbd97b4f2ef59dbd` | 15–76 |
| .github/workflows/release.yml | 2026-09-12T11:38:48.492348+00:00 | 3779 | `caaf9eb891dc6636229ed9a07e3474d6f3a8823f6670d2a3b4d2954cce0d9cbd` | 13–57 |
| tools/release.sh | 2026-09-12T11:38:48.492468+00:00 | 2222 | `78b395354e9c404488d17e9c8dd4c1c9c30fcd65ec926e7c61e5eb51df521094` | 1–49 |
| tools/bench_runtime.py | 2026-09-12T11:38:48.492594+00:00 | 15327 | `ef6701f52659c78f58fb3f31adf87343c31b34229145f57acba2be05b1a9a52c` | 24–40, 130–139, 250–270 |
| Cargo.lock | 2026-09-12T11:38:48.492909+00:00 | 46021 | `7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348` | 41–46, 204–219, 487–492, 601–616, 675–680, 909–921, 1105–1114, 1135–1147, 1157–1167, 1295–1307, 1308–1316, 1370–1384, 1421–1426, 1438–1443, 1450–1469, 1674–1686, 1710–1731 |

## 接收、静态差距与回退

可确认的是根manifest完整声明及其精确增量，与选定锁块/调用/历史来源相符；不能确认的是全图feature激活、Rust1.88真实兼容、全部target构建、完整资源/composer/editor行为、panic终端恢复，以及任意后续release预算。现有静态关注点是默认feature解释、target计数口径、event-stream对reset API的条件冲突、fixture与production profile区别、local patch源身份及SBOM表达范围。它们是维护和验收边界；本轮没有把未运行的组合写成已复现故障，也不因包机械校验通过宣称G-FILE已获主控接受。

master.patch与worker.patch只从/dev/null新增标准学习报告，两者内容一致。每个接收模拟先在新建临时目录git init，再git apply --check/正向apply/反向--check/反向apply；正向目标逐字节和SHA256匹配report，逆向回到absent，非目标sentinel原字节不变。ready包含报告、证据、两份patch、README与总manifest；离线verify_package只依赖包内副本、Python标准库与临时Git，不依赖主库、旧ready、Cargo、网络或产品二进制。manifest校验防篡改并列出artifact全集，不能自我替代语义审查。

回退仅反向撤本报告新增，不删除旧候选/ready、不覆盖产品或用户会话。发现主库报告由absent变成已有文件时必须由主控对新真实基线重做差异，不能把这里的new-file patch强行用于不同接收状态。097保持worker candidate，等待主控独立G-FILE/G-STAGE；不代他项标记完成。

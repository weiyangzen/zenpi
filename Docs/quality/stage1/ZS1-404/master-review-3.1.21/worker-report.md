# ZS1-404 — vendor/crossterm 独立目录理解候选

唯一正式产物为 `Docs/learn/stage1_pi_mono/targets/zenpi/vendor/crossterm/current_folder_learn.md`，understand模式，产品实现LOC0。蓝图3.1.21的404精确路径是vendor/crossterm，只Depends ZS1-403；requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`和baseline `92b06c4b1dcdca7614d226ce5f41205646a9e63967789d2b6ed4ee272d240884`保持。初始402、403均未被主控独立接受、receipt不存在，已保留原authority与缺失事实。本轮先完整准备候选，最终状态以封包记录为准；不升级404、405或root验收。

本目录职责是被Zenpi通过Cargo patch引用的crossterm crate包边界：声明库入口、依赖和features，保留第三方说明/许可/开发材料，承载定制src。它不是另一个事件循环owner，也不因包含源码、examples和CI配置就表示整个crate语义或所有配置经过验证。G-DIR仍要求唯一正式子项403先独立接受，再审阅本目录连接；文件清单与hash只能证明所讨论的字节身份。

## 直属物理库存与冻结子集

本轮直接枚举含隐藏项，实际九个普通文件和四个普通目录，无symlink、设备项或额外隐藏成员。

| 直属项 | 字节/总行数 | 角色和读取范围 |
|---|---|---|
| .cargo_vcs_info.json | 94 B / 6行 | 完整新读；上游打包VCS元信息，不是当前定制代码commit证明 |
| .gitignore | 63 B / 5行 | 完整新读；开发忽略规则，不删除本地文件也不证明未纳入包 |
| .travis.yml | 1004 B / 42行 | 完整新读；历史CI配置数据，本轮不执行其命令 |
| CHANGELOG.md | 36915 B / 794行 | 只完整读必要1–95行，覆盖0.29到0.26开头的相关演进；其余不声称已读 |
| Cargo.lock | 29738 B / 1156行 | 读取header及本包/事件后端相关完整package块，精确范围见read-blocks；不声称读全锁图 |
| Cargo.toml | 4455 B / 240行 | 复用400起已完整读取的标准化清单，当前字节完全相同 |
| Cargo.toml.orig | 3772 B / 122行 | 完整新读；与标准化清单比较package、lib、features与依赖意图 |
| LICENSE | 1083 B / 21行 | 完整新读并保存原文；MIT声明、copyright及条件/免责声明原样保留 |
| README.md | 8818 B / 222行 | 完整新读；能力/用法/依赖说明，发现旧版本和feature描述滞后，不作为当前行为权威 |
| .github/ | 普通目录 | context-only；直属CODEOWNERS、ISSUE_TEMPLATE/、workflows/，只清点，未运行工作流或接受目录 |
| docs/ | 普通目录 | context-only；直属.gitignore、CONTRIBUTING.md、三个图像资产、know-problems.md；只完整读必要两份文字文档，图像不作语义阅读 |
| examples/ | 普通目录 | context-only；README与10个rs文件，无子目录；完整读README并核对清单目标名，未声称10个示例源码全读/运行 |
| src/ | 普通目录 | 唯一正式直接子目录ZS1-403；其真实10文件4目录与独立403候选对应，不把孙项event.rs/099提升为本目录直属项 |

file index在vendor/crossterm的冻结直属文件数为0；folder index直接子项只有403，.github/docs/examples无本分支正式目录项。闭包仍为099→400→401→402→403→404→405(vendor)→091(root)。已接受400/401和099不允许跨过402/403；全局其它054/406等接受进展不改变本条唯一依赖。

## 完整阅读、范围阅读与复用证据

prior403整包不变，manifest `e823581c4325d8c6b0a6daa3facc9fda4c735872155779617145465a9966fed5`，其中保留400–402原包、099完整读取和已接受400/401凭据。403的8份完整新读与25份完整复用，本轮逐字确认后共复用33份完整context。mio仍25801 B / 655行 / `41321e242e21fa85533dbae42314bcf80ab916e1d55e065d5ce98076002ac93f`，原57f4→1afa→41321e六段连续读取链照原证据保留，冻结099 source_hash不改成实现hash。

本轮新context共12份：9份从0到EOF完整读取（上表六份新完整直属文件及docs/know-problems.md14行、docs/CONTRIBUTING.md65行、examples/README.md40行），共537行；另外CHANGELOG1–95、vendor锁9个连续片段、根项目锁7个连续片段按必要范围阅读。合计26个块、818行，范围绝不冒称全文件。保存全文件快照只是便于校验范围/上下文身份，不意味着读过其余字节。范围清单在new-context-read.json，逐块byte offset/hash在read-blocks.json。

新读Cargo.toml.orig与旧完整Cargo.toml语义对照，比单纯hash比较多说明一层：哪个文件定义实际构建入口，为什么同名包的独立锁与宿主锁可以选择不同版本，features如何映射到已理解的src。README及CI中的命令是待分析文档，不是本轮执行指令；没有因贡献指南要求consult CI而运行任何外部流程。

## 包身份、标准化清单和库入口

两个manifest都声明crossterm0.29.0、edition2021、rust-version1.63.0、MIT、lib名crossterm及入口src/lib.rs；这些是包声明，不等于本轮实测MSRV或所有平台兼容。Cargo.toml头部明确是Cargo上传时标准化生成，orig保存作者配置。实际构建读取Cargo.toml；仅编辑orig不会自动改变当前标准化文件，本轮两者都不改。

标准化清单明确build=false，关闭autolib/autobins/autoexamples/autotests/autobenches，显式列出lib和10个example路径；本目录物理无build.rs或顶层tests目录。由此描述本快照的根级入口，没有推断所有依赖都无build script，也没有把src内#[cfg(test)]模块说成因autotests=false而禁用。orig列出的9个带features示例与标准化文件保留的is_tty等显式目标需要按最终清单看，不能单靠文件在examples/下就认定任意文件自动成为target。

package.exclude与.gitignore都列Cargo.lock/target等，但本地实际存在Cargo.lock；这些规则是打包/开发配置，不能倒推当前库存不存在该文件。docs.rs metadata all-features=true描述文档构建意图，不是当前Zenpi默认features，也不证明全feature组合实际通过。保留LICENSE原字节及上游署名是本目录材料的一部分；报告不改许可或替用户做新的分发结论。

.cargo_vcs_info声明上游git sha1 `36d95b26a26e64b0f8c12edfe11f410a6d56a812`和空path_in_vcs。它不随本地event.rs/mio定制自动变化，因此不能作为当前实现等于上游commit的证明。蓝图此前固定0.29原始crate SHA与77归档成员的记录属于既有完整性基线，本轮未重新下载、解包或声称全77项语义读取；当前输入链的定制字节以099及父目录快照为准。包版本仍0.29并不隐藏或消除本地定制。

## feature、依赖及锁文件的真实连接

| 清单条件 | 依赖与src连接 | 语义边界 |
|---|---|---|
| default | bracketed-paste、events、windows、derive-more | 根Cargo没有禁用默认，Unix目标仍走mio而非Windows实现 |
| events | dep:mio、signal-hook、signal-hook-mio | lib.rs开放event，后端提供读取/readiness/SIGWINCH；无events则不公开该模块 |
| event-stream | futures-core并包含events | 启用后台等待线程/双waker；不意味着自动采用tty替代后端 |
| use-dev-tty | filedescriptor、rustix/process | Unix source和waker选择器同时切至tty；不会自动从events列表移除mio/signal依赖，也不是初始化失败fallback |
| libc | 可选Unix依赖的隐式feature | 默认Unix使用rustix；根项目直接依赖libc不会自动启用crossterm libc feature |
| windows | winapi、crossterm_winapi等目标依赖 | 还受cfg(windows)限制；lib.rs在Windows关闭该feature会compile_error |
| serde | dep:serde、bitflags/serde | 类型编码功能，不建立session持久化owner或磁盘协议 |
| derive-more | derive_more is_variant | 类型辅助方法，不改变输入来源/互斥合同 |
| osc52 | base64 | 启用clipboard模块；非当前默认输入链，不随本目录接受 |

bitflags、parking_lot、document-features等提供类型标志、锁、文档能力；Unix rustix总依赖用于stdio/termios，mio/os-poll、signal-hook-mio/support-v1_0与可选libc/filedescriptor各有条件。dev-dependencies包含async-std、futures/timer、serde_json、serial_test、temp-env、tokio/full，用于示例/测试，不是仅因存在于库清单就全部成为Zenpi生产runtime组件。没有执行Cargo tree或全features解析，以上来自已读manifest与锁的指定块。

| 与事件后端相关的锁项 | vendor独立Cargo.lock | Zenpi根Cargo.lock |
|---|---|---|
| lock格式 | version3 | version4 |
| crossterm | 0.29.0，包含独立开发/可选依赖的图条目 | 0.29.0，根patch路径库条目；该条目无registry source/checksum |
| mio | 1.0.3 | 1.2.3 |
| rustix用于crossterm | 1.0.5；锁中另有0.38.44 | 1.1.4 |
| signal-hook | 0.3.17 | 0.3.18 |
| signal-hook-mio | 0.2.4 | 0.2.5 |
| crossterm_winapi | 0.9.1 | 0.9.1 |
| filedescriptor | 0.8.3条目 | 本轮根锁相关扫描未找到该package条目 |

锁记录已解析版本/source/checksum/依赖名，不记录每次构建全部feature激活事实；不能从锁内出现某可选依赖推导当前必编译执行。宿主根Cargo中crossterm0.29+bracketed-paste、[patch.crates-io]指向vendor，说明根项目构建以自己的锁和图集成；vendor内锁不会覆盖宿主图。已有099实际vendor默认8/libc-stream9与主控根集成测试本就属于不同源图/配置，不能把同一crate版本号当作依赖二进制完全一致，也不能把8+9简单累加成17个独立证明。

本轮仅阅读两个锁的header及完整命名package块，未审阅全部传递依赖、checksum网络真实性或所有feature组合，更没有重新解析/更新lock。根锁其它无关包的并行变化不要求回写旧400–403包；涉及当前选定的依赖边界则需按真实字节说明，不能旧hash盖章。

## 唯一正式src子项的语义整合

src/403候选提供crate public模块与Command输出层接线，event/402候选提供共享reader/过滤/解析/query/reset边界，已接受401/400/099提供平台source与Unix就绪/ESC修复的理解。本包的整合是解释manifest如何使这些owner进入构建，而非重做或跳过各目录接受。

库入口lib.rs导出Command/Queueable/Executable/SynchronizedUpdate，event受events门控。输入由static reader锁、reader过滤队列、source Parser/readiness各自持有；默认mio41321e保留未消费token、部分字节和已完成事件，FIONREAD避免已排空blocking fd再读，idle helper只完成确认单ESC。Windows与use-dev-tty的差异仍为必要context，不能因为根features选择能力就视为实测平台验收。

输出由调用者Write接收ANSI或Windows命令。queue不是crate自有延迟对象队列；execute再flush，部分失败无回滚。sync_update Begin/closure/End不是RAII事务，panic不保证End，Command Display在部分Windows路径可有副作用。包根没有统一屏幕/输入资源管理对象，将library作为依赖不会自动帮宿主恢复raw、光标、alternate screen或持久化草稿。

raw由terminal的独立登记管理，query与普通event共用reader；public read/poll要求受支持的消费方式，EventStream另有线程/waker约束。reset只在Unix且not event-stream可用，try_lock失败WouldBlock、成功销毁内部reader状态，不flush OS输入、恢复termios或join线程。mio EOF、tty 0读取与Windows空queue不同，parse None/Err与io错误不同；包含两条后端实现不建立统一EOF/取消/恢复保证。

本目录清单/README/metadata不拥有项目ID、session、HTTP、WAL/JSONL writer或close hook。errors由src层按各分支返回或忽略；Drop、raw恢复、输出flush、输入wake都是不同动作，没有包根事务把它们合成“用户请求已保存/任务已停止”。该边界是403链中职责在crate包层的表达，不能从Git包完整性推导durable recovery。

## 开发材料、示例与发现的文档滞后

README完整读到两个Cargo示例仍为0.27，而实际两个manifest都是0.29；feature表将filedescriptor写成替代mio，并将libc关联events，实际当前代码按use-dev-tty选择source/waker，默认Unix依rustix。已先向主控报告这一资料不一致，未改README或产品；以manifest和实现决定实际接线，不以旧宣传说明推导当前能力。README所列终端测试/Windows7等只是上游文档陈述，不是本轮或Zenpi定制版本的跨平台实验。

examples/README还提interactive-test和interactive-demo子目录，实际本目录只含README与十个rs文件，没有这两个子目录；这是可复核的旧指南滞后。实际Cargo.toml为copy-to-clipboard要求osc52、若干event示例要求events/bracketed-paste、两async示例要求event-stream/events，is_tty没有required-features；声明示例可构建的条件不意味着执行成功。本轮未新读十个示例源码，也不把既有原库tests直接当作应用验收。

.travis.yml的历史矩阵列stable/nightly、linux/windows/osx、nightly允许失败，script有fmt/clippy/build及串行lib/default/serde/event-stream/all-features测试/package等。读取配置只说明开发意图，不证明当前CI启用或最近结果，更未执行这些命令。.github工作流/issue模板仅库存边界；不把vendored的CI配置视作主项目实际CI门禁。

docs/CONTRIBUTING的导入分组、格式长度和warning规则是上游贡献指南，不覆盖本轮用户明确的只读产品/只写报告授权。know-problems记录PowerShell/终端颜色、启动输入与KDE resize等历史观察，文档归因不等于当前故障原因或本次实测。CHANGELOG必要段落提供0.29 rustix更新、0.28.1 mio/signal对齐与0.28默认rustix的演进背景，但没有逐版本重跑验证，也未读取95行以后的全历史。

## 证据时点、未通过范围和交接

33份完整上下文、原包/receipt、当前mio身份与冻结依赖映射均独立核对；精确reuse保留原失败和反证。旧socket失败、公共ESC PTY前后均通过、144/68 Rust、6891/31ea以及后来的主控review背景各有原身份，不能因crate版本都为0.29就混合为当前全量通过。本轮无产品/build/PTY/runtime/budget执行，无旧runner或历史verifier执行，不重封旧400–403；新离线验证器仅检验读取范围/字节关系、清单/库存、依赖状态和唯一404补丁。

402、403在初始与所读403候选中尚未独立接受，404不能越级完成G-DIR；若接受状态后来改变，需另附主控真实receipt和review，不抹去旧时点。本包accepted=false，405/vendor与root以及.github/docs/examples都不随它接受。新发现仅是上述文档/真实库存差异，未将它们升级为实测产品回归或越权修复。

全部写入新私有.ops，主库/构建输入/claims/authority/旧ready/BentoBox不动。worker HEAD和94条non-.ops dirty保持。回滚仅撤回404报告，不递归回滚子项或覆盖产品、session和许可文件。封包后记录外置只读校验回执，交接后停止等待，不开新task/session/subagent。

## 封包时点与读取范围索引

权威固定于 2026-09-12T17:55:35.281433+00:00，封包 2026-09-12T17:58:43.973798+00:00，snapshot `8759df3d849cb5bb7dcd1c31d7ac624b7cec594ffd240ec0f009989ff23f885c`。402 receipt存在=False，403 receipt存在=False，404 accepted=false；这不是G-DIR通过声明。后续并行接受进展需由主控另附实际凭据，不改initial/旧ready。正式404报告尚不存在，相关完整读取与所读必要范围已核对。

|新context|本轮读取行范围|全文件字节/SHA（仅身份，不等于全读）|
|---|---|---|
|vendor/crossterm/Cargo.toml.orig|1–122|3772 / `a4db652d1a5e748e0da1c3efc0e7baf3d969d7b376dd773183c308ea721a9f7e`|
|vendor/crossterm/.cargo_vcs_info.json|1–6|94 / `c36e7cbcffc6c12772c1e6f9205c9eeb139af7d9dd702f9dcdb266e8f03d43ad`|
|vendor/crossterm/.gitignore|1–5|63 / `187cd13134be31e6fa457b83bae023129b77a81bf3c0a44bd954ec05de58afe5`|
|vendor/crossterm/.travis.yml|1–42|1004 / `37db6d13f57492a2fe5e3707c486dd9c66d919e9b48544321cbae66bde77f138`|
|vendor/crossterm/LICENSE|1–21|1083 / `aca4760f2a5eba9ce9d4448fe7cd3fcf98245a8315ede59ebd0c6085dd542e61`|
|vendor/crossterm/README.md|1–222|8818 / `7c812f101532cf858f43e8abe2bd2fcc4d76cd0599db9f90722d493ef0049eff`|
|vendor/crossterm/CHANGELOG.md|1–95|36915 / `40ffad2095ee87a178b29ab9f7b190f754bf4f02c8f791c231fc5a9702e3d452`|
|vendor/crossterm/Cargo.lock|1–4, 230–257, 258–266, 340–350, 566–577, 693–705, 706–718, 809–818, 819–829|29738 / `c05ea83405bfb2e4238460213ccd3e8a4c793b1bdaec3405fbcff894093e9243`|
|Cargo.lock|1–4, 204–219, 220–228, 760–771, 1024–1036, 1174–1183, 1184–1194|46021 / `7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348`|
|vendor/crossterm/docs/know-problems.md|1–14|672 / `e74dc9f17bfeb1cfc911f5e8c7ad469feeaab5b2223db09bb5cf1ebc49bc7cc4`|
|vendor/crossterm/docs/CONTRIBUTING.md|1–65|2072 / `fc6ff43426735c16f2d061f0f791c84885a84f1cba317b6cdf706e068d3282e8`|
|vendor/crossterm/examples/README.md|1–40|1489 / `861e27b9390b6e8c0c2d793c2cb572c71bf8d75cce5cbdd0715b6fd01c55932f`|

|精确完整复用context|行数|完整字节范围|SHA256|
|---|---|---|---|
|vendor/crossterm/src/command.rs|295|[0,10943)|`0cefa31bc00f5e1f11fdae16c6031fe46d397bb8c1990cf55dc3b9e83d29e3ce`|
|vendor/crossterm/src/macros.rs|378|[0,12627)|`cb09a33ea7affaad7171b5f2325bbc78aad041cd719450a1b6d44e07793c215e`|
|vendor/crossterm/src/cursor.rs|504|[0,15481)|`0d4ed77fca1b318d0c954cc01fb6694d72b0fa3e09bc24dd88b69ab565616007`|
|vendor/crossterm/src/terminal.rs|568|[0,18102)|`01af7ff59a6a21090da0316b2688d41dc8dd99425413e1eacd3494065b0449d8`|
|vendor/crossterm/src/tty.rs|54|[0,1594)|`9452b86a2dbaa35eadab5615aa9eed131954bcf2ed9e1f83e7c5b73e57a9bdc8`|
|vendor/crossterm/src/ansi_support.rs|46|[0,1834)|`36b902490d60259d99f88a2b45555d2d9d13f2c344cb3693c6a81442670f632b`|
|vendor/crossterm/src/cursor/sys.rs|20|[0,551)|`447f839bb2968a808f8d5bd763516a1bff9024e38fe4d852c57f3cdb4ee070e1`|
|vendor/crossterm/src/terminal/sys.rs|27|[0,755)|`b77c654d581eb939b00e77d565d950c2f6045eb9b64480b21c1b20538665bb14`|
|vendor/crossterm/src/event.rs|1777|[0,63629)|`989c472a11a6b471e663e191fd64337593423de2aa4774344cb246fb975bbd42`|
|vendor/crossterm/src/event/sys/unix/parse.rs|1506|[0,55081)|`689b765d67be62703f7935abfa1475d98994987461e413836b54c7936966ce2b`|
|vendor/crossterm/src/lib.rs|263|[0,10739)|`ec896da229e4b9b02da3cf444bf1d89e0bc8dc900a29ded2b2a1df086d92b6aa`|
|vendor/crossterm/src/cursor/sys/unix.rs|56|[0,1711)|`e048206557e41c2090437d2b2b30c0485d17db9bb90e726e44d2e8d3ec5d5d63`|
|vendor/crossterm/src/terminal/sys/unix.rs|326|[0,10684)|`db066f11aa75d6c06515fd600ee38336f230d0f8f9d307663debb6ba835b334b`|
|vendor/crossterm/src/event/source/unix/tty.rs|278|[0,10162)|`453e854ca82cd2d37e57494d9766a9eea728883a8a1c5fec186f7c6b83dbd64e`|
|vendor/crossterm/src/event/source.rs|27|[0,925)|`f606a124266b6d86b58969348bb713f7f89fb09886f51bfb6fe16b766e113bb7`|
|vendor/crossterm/src/event/source/unix.rs|11|[0,294)|`253ed074fbe63e7270fc19e5c85186407c3a91454f43291c29cf5717575c74a6`|
|vendor/crossterm/src/event/sys.rs|9|[0,248)|`3ec336b2d4c36cfc895fc7cf586e92f7763473130ca99e8168bead1ba3918d1a`|
|vendor/crossterm/src/event/sys/unix.rs|5|[0,110)|`d651928295213cd12a4df7edeb0c8be5a4e4487345fc9f2c2ff59d6411047cf2`|
|vendor/crossterm/src/event/sys/unix/waker.rs|11|[0,258)|`fe7a1e77718306e9f84b77337f4e9ea9990bed4cf533036228508316363add0e`|
|vendor/crossterm/src/event/sys/unix/waker/tty.rs|28|[0,685)|`49132cd71d20b7cc27b79d28a490743da77241bed24a178e22d9ad6e5e98e185`|
|vendor/crossterm/src/event/sys/unix/waker/mio.rs|34|[0,1026)|`cbc89c146a004ad689db7f17e3cfc9ce6198c115f31e519d25c68a441db3ecb4`|
|vendor/crossterm/src/event/read.rs|436|[0,14606)|`704eb054bff7408e7591e7b5869cf4606794a43b69dc7a1edef67fa7e26ccb8a`|
|vendor/crossterm/src/event/timeout.rs|92|[0,2662)|`cbe518ad2cb666b588f593b4b84704eb4480e49d9bd3f28c72d696dae6e7b5e7`|
|vendor/crossterm/src/terminal/sys/file_descriptor.rs|154|[0,4249)|`d215ff60394ec2150c2df6340343d84a3922570c1feba813663742d0eb85d13f`|
|vendor/crossterm/src/event/filter.rs|115|[0,3786)|`092515c0939799af80768e7b87d2ea4ffc5b1bcda021982f6f02922378809df5`|
|vendor/crossterm/src/event/stream.rs|146|[0,5092)|`1df7f35a1842d54b6a8d2b82c96661305d7bd87bbdf123ce6670c62fec5f3cb1`|
|Cargo.toml|50|[0,1228)|`d5fafd029ef77454e178222c2b62530d28859374f72a2ce6bd972a465bed5884`|
|vendor/crossterm/Cargo.toml|240|[0,4455)|`78b5e8b5cd6660067195979533e61d4b33c375c74613f24180efd94c86f7d295`|
|vendor/crossterm/src/event/source/windows.rs|100|[0,3656)|`e25bdd13432dfda96401a76f3e2d6aa2a404fbaa4f1b99b55bb0bdec2ac75336`|
|vendor/crossterm/src/event/sys/windows.rs|48|[0,1707)|`130d7da62a0af8b1cf7c55bd8bf78fa0272a9a44803704dac6416db993d30d81`|
|vendor/crossterm/src/event/sys/windows/poll.rs|86|[0,2545)|`232c69f9c9aa058189a418cdaee82134feaf6ef91f89cea8eaaeb5b4cfd722fc`|
|vendor/crossterm/src/event/sys/windows/waker.rs|40|[0,1156)|`a145d8ccf6b7747eb4e9499bdade2616a997539da4f4691f4c14bd4c4d6a472b`|
|vendor/crossterm/src/event/sys/windows/parse.rs|378|[0,15566)|`15afbd3b84edfa2a72f9587f591ce9f7dfe99f1cb85db8b45037c0b70c0346ae`|

# ZS1-080 — src/extensions.rs 完整理解报告（3.1.21，候选 [_]）

当前 main `src/extensions.rs` 已连续逐行读完 1–626 行，三个实际读取区间为 1–250、251–500、501–626，覆盖全部生产定义、辅助函数和错误类型。**源内测试声明为 0**，没有因文件较短而用旧报告、函数索引或摘要代替全文。本轮另读完 `tests/extensions.rs` 的 180 行、5 个测试及 fixture helper，并读了 32 段必要上下文。产品执行 **0 次**；正文所列测试断言和后续验证条件**全部未运行**。

唯一 owned 是 `Docs/learn/stage1_pi_mono/targets/zenpi/files/src/extensions.rs_learn.md`。捕获时 main 此报告不存在；本包正文独立成篇，forward.patch 只创建该报告，reverse.patch 只删除该报告，均未应用。旧报告及失败完整保存在 prior，不把其旧时态拼接为当前事实。状态与语义接受由主控负责，本轮不改 main、产品、测试、authority、claims、receipts 或旧 ready。

| 身份 | 字节 / 行 | SHA-256 |
|---|---:|---|
| 当前 source，本轮全文阅读 | 20742 / 626 | `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed` |
| requirement 冻结 baseline | 23071 / 685 | `9b1879edcfb70e711960092e12060a9137e2aacda6bdc547318e5ccb7510fe39` |
| 旧 3.1.20 包中的 current source | 20742 / 626 | `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed` |
| 当前 tests/extensions.rs，本轮全文阅读 | 6302 / 180 | `00a3873257e010a9fdd337d8b913fda3f2efe101710c45ae225975ab59c2237d` |

Authority 3.1.21；requirement digest `3456abcbbebbc4e0ab383c319851b0a6e71b19ee9b3c060a6e212f61a89c9d9d`；assignment 53/121、snapshot `3c01628b900c9078f7c1771f3638cc366b2e295c6fede7cfea191aa0620c5c2d`。主控通知 086 已独立接受、render 窄屏 emoji 的 3 个反例正在修复和构建；这些不是本轮执行或结果。本轮不读取 render 作为接受依据。正式依赖仅 ZS1-001，保存其 accepted receipt 和 34 个 artifact 身份；依赖接受不传递成本项接受。

初始 capture/chunk-plan/context-plan 的 false 留作捕获状态，reading-ledger 另记实际完成。当前 source 与旧 current 相同，baseline 比当前多 2329B/59 行；baseline-to-current.diff 已完整读，历史到当前 diff 为空。没有宣称本轮重新全文阅读 baseline 或相邻 owner。以 execution-cron-builder 的 worker/master 分工和 learn-cron-builder 的 understand 单文件映射执行；本次授权限定为候选报告，覆盖规则不扩大为改目录报告、创建自动任务或运行产品。route/estimator/read-note 只存在私有包内。

## 1. 数据契约、常量及校验（1–158）

模块管理安装目录、清单、工具注册及独立的 legacy capability broker。常量 `EXTENSION_MANIFEST=extension.toml`、API 1、hook API 2、工具 frame 1MiB 约束不同层次；它们不意味着安装树总量有界或本地进程是沙箱。

`ExtensionManifest` 严格拒未知字段，name/version/api_version/executable 必需；disabled 默认 false，args/tools/hooks 默认空，permissions 全 false，`default_hook_timeout()` 返回 1000ms。`ExtensionPermissions` 的 read/write/command/network 独立；`ExtensionToolManifest` 要求 name/description/input_schema/side_effect，也拒未知字段。`ExtensionSummary` 保存名称、版本、disabled、compatible、directory 展示路径、工具名称及权限，不含 hook 列表；summary 的反序列化类型没有同样的 deny_unknown_fields。`LoadedExtension` 保存 manifest、canonical directory/executable，crate 可见。`ExtensionCatalog` 的 enabled 集合是 BTreeMap，summary 则是独立 Vec。

`ExtensionManifest::validate` 依次验证 name/version、API 1 或 2、relative executable、args；错误的 supported 字段为最高支持版本 2。args 至多 64 项，各至多 4096B 且无 NUL；未拒空 arg。必须有 tools 或 hooks，tools 至多 64。API 1 不得有 hooks，hooks 至多 8、无重复，类型由 HookKind serde 限定；timeout 必须 1..1000ms，**没有 hook 也要满足此范围**。API 2 只有 hook、或只有 tool 都可合法。

每个 tool 先验证 identifier，再构造 ToolDefinition 并调用其 validate，拒同扩展内重名与未声明的 side effect。共同工具校验负责 description/object schema 等定义边界；`input_schema` 在这里是定义资料，非对未来每次工具参数运行完整 JSON Schema。manifest 本身不读取可执行内容，不检查 Unix execute bit，不验证 argv 被实际程序如何解释，也不做网络隔离。工具名字的扩展 ID 语法最大 128B，但共享 ToolDefinition 仍可施加更紧 name 限制，不能只看第一层校验。

## 2. Catalogue 的加载、排序、路径与资源边界（162–267）

`load` 遇不存在 root 返回 empty default，现存非目录返回 Path；root canonicalize 后读直接 children，`filter_map(Result::ok)` 静默丢弃 read_dir 单 entry 错误，先收集全部 path 再排序。UTF-8 点号开头名称跳过；非 UTF-8 filename 不满足该条件。symlink child、非目录 child 跳过，其它 metadata/canonicalize 错误传播。canonical directory 必须在 canonical root 内；root 本身可以通过 alias canonicalize，不是全路径一律拒 symlink。

每个 directory 下不存在或非 file 的 manifest 跳过；is_file 之后再 symlink_metadata，manifest symlink 报错。随后 File::open、take(256KiB+1)、read_to_string；只读到上限加一且要求 UTF-8，长度过限在 TOML parse 前拒绝。上限位置正好切断 UTF-8 时可能先返回 IO/InvalidData。不同于后面的 install/upgrade/set_disabled，这里确实对原始读取加 take。

解析后先构造 summary。disabled 项直接进入 summary，绕过 validate、executable resolution、enabled 重名和 32 个 enabled 上限；但它仍须是能完整反序列化的严格 TOML，未知 HookKind 等仍会拒绝。compatible 仅表示 API 数字 1/2，不证明 disabled 项的其它字段能运行。enabled 项依次 validate、join executable 后 canonicalize，要求在 directory 内且是 file；内部 executable symlink 若最终仍在本目录会允许。没有 executable inode/content hash 或权限位固定。

enabled 的 manifest.name 重复报错；目录名可与 manifest.name 不同。最多 32 enabled，disabled 数量、summary 总数、root 子项 Vec 没有同样总量 cap。summary 顺序按目录路径，后续 loaded.values() 顺序按 manifest 名；两者不能混同。32×64 限制声明数，不限制复制树的总字节或运行时实际副作用。

metadata、canonicalize、open、后续 spawn 是不同时间点；这些路径检查没有目录 fd/CAS，硬链接、替换竞争不由检查顺序自动排除。程序路径在 load 后被覆盖也没有自动重新校验摘要。`summaries()` 返回不可变 slice，不能借返回值变更 catalog；其中 path 是 display 字符串，非独立的安全授权。

## 3. 注册与真正工具执行（269–357）

`register_tools` 只要遇任意 enabled API 2，便在修改 registry 前返回“API 2 extensions require a session runtime”；hook-only API 2 也不例外。纯 API 1 调用 `register_with_lease(registry,None)`。后者 clone 原 registry，在 candidate 中依 manifest 名、声明工具顺序插入 LocalProcessTool，全部成功才 `*registry=candidate`。跨扩展/已有工具重名的晚失败不留下部分插入，这是 baseline 的实质修复；它不是磁盘或 journal 事务。

LocalProcessTool 保存 definition、整个 LoadedExtension clone、可选 ExtensionLease clone。`definition()` 返回 clone；`invoke` 以恒 false 取消函数委托 `invoke_cancellable`；后者再次按声明 effect 检查 manifest permission，失败 PolicyDenied，成功交给 `extension_runtime::process_request`：method=`tools/call`，params={name,arguments}，workspace 取 ToolContext，timeout 固定 30 秒。**hook_timeout_ms 不限制工具调用**。未 override execution_mode，故依 Tool trait 默认 Sequential；ReadOnly 名义不使扩展工具自动并行。

本轮阅读 tools consumer 1000–1067、1125–1408：register_boxed 标记 builtin=false；execute_inner 验证 call、取消、capture call_id、lookup、SideEffectPolicy、gate、worker builtin 身份，才 invoke_cancellable，之后脱敏、policy_evidence 和序列化输出上限。普通 execute 不调用 validate_hook_schema；只有 hook rewrite 路径的 validate_hook_call 做严格有界 schema 子集和 builtin path 校验。因此不能声称所有扩展参数都由 host 验证了完整 schema，可信扩展必须自己处理参数契约。

manifest permission 是声明 effect 的前置准入：ReadOnly→workspace_read，WorkspaceWrite→workspace_write，CommandExecution→command_execution，互不继承。network 字段在此映射中无分支。允许本地扩展后，进程拥有当前 OS 用户权限，不会因 workspace_read=true 或 network=false 变成只读/断网。直接 API 1 catalog 注册是可信 embedder 入口，不经过 core 生命周期和人工审批流程；通过真实 Agent 的工具调用则受其 registry/approval/worker 防线。扩展 hook 本身在工具审批前或生命周期时也会启动本地程序，不能把“待审批的模型工具未执行”扩大为“任何扩展进程都未启动”。

公开 registry 的 preview 对一般扩展名返回 None；代码按 write_file/edit_file 名称分派，未额外查 builtin。完整内置 registry 中同名扩展因注册冲突拒绝；自建空 registry 若允许这些名字，preview 仍会按内置文件语义生成，不能认作扩展程序的真实副作用预览。077 已记录此边界，本轮不修改产品。

## 4. 安装、升级、禁用及失败后的磁盘状态（359–469）

| 函数 | 正常顺序 | 具体失败边界 |
|---|---|---|
| install | source.is_dir→无 take 读 manifest→parse/validate→create_dir_all(root)→检查 destination 与 `.name.installing` 不存在→copy_tree→rename→load 整个 root→first matching summary | copy/rename 失败可留 staging；rename 后 load 失败仍留下已发布目录，没有通用回滚；其它坏扩展也能使全 catalog load 失败。没有 lock/fsync/cancel。 |
| remove | validate_id→path.exists 否则 false→remove_dir_all→true | 不加载 catalog，不撤销已经捕获的 runtime/lease；删除后的错误/部分删除不能仅凭返回类型理解为未变。dangling path 受 exists 语义影响。 |
| upgrade | source/manifest validate→destination.is_dir→拒 `.name.upgrading`/`.name.backup`→copy→old rename backup→staging rename destination→load→remove backup→first matching summary | 第二次 rename 失败只尽力恢复 backup；load 失败尽力删除新目录再恢复，回滚错误都忽略。copy/第一次 rename 可留 staging；backup 删除失败发生在新版本已发布后。无 crash-durable 事务。 |
| set_disabled | validate_id→manifest.is_file→无 take read/parse→相同值 false→修改 bool→pretty TOML→固定 `.toml.tmp` write→rename→true | 没有 manifest.validate、catalog recheck 或 catalog 式 symlink 拒绝；可启用随后 load 会拒绝的内容。固定临时路径不是 create_new；失败无统一 cleanup/fsync，格式与注释不保留，也未承诺 mode 保留。 |

install/upgrade 的首轮 validate 不固定后续被复制的 source；rename 后再次 load 才看最终清单。source 顶层 is_dir 会跟随别名；copy_tree 的 entry symlink 拒绝不是对所有祖先和使用时点的防护。source/root 不要求互不嵌套，递归没有检测目标位于 source 内的情况；这是需要另行验证的输入边界，本轮没有构造或运行该场景。

`copy_tree` 先 create_dir 当前 destination，按文件系统顺序读 entry，file_type 后 directory 递归、file fs::copy，遇 symlink 尽力 remove 当前 recursion destination 再返回 Path，其它特殊文件类型跳过。深层失败不会保证清掉外层 staging；普通 IO 早退也无 cleanup guard。没有总 bytes、entries、depth 或取消预算，没有 ignore 规则；file_type 与实际 copy 之间也没锁定对象。不能把清单 256KiB 上限推导成安装树有界。

目录按文件夹存储、summary 按 manifest.name 选择；disabled 同名项可同时存在，install/upgrade 的 `.find(name)` 可选到目录排序更早的同名 disabled summary，不能把返回 summary 当成目标目录身份的原子证明。磁盘 install/remove/enable/disable/upgrade 与 live Agent registry 是两套状态；已加载 runtime 保存路径/manifest，只有显式 reload、替换工具、resume/close 等 host 流程才处理其 lease。磁盘修改也可能让旧路径内容变化，不能把仍 active lease 理解为代码字节仍未变。

## 5. 词法 helper、legacy broker 与错误（471–626）

`permission_allows` 做三 effect 的独立布尔选择。`validate_id` 允许 1..128B ASCII alnum/下划线/横线，允许大写和数字开头；`validate_version` 允许 1..64B ASCII alnum/点/横线/加号，不解析 semantic version，不保证版本递增。`validate_relative_component_path` 拒空、>4096B、absolute、ParentDir/RootDir/Prefix；CurDir/普通组件允许，没有显式 NUL 校验。真正 canonical/type 检查属于 enabled load，词法 helper 不产生文件系统行为。

CapabilityScope 的四种值 read/write/command/network 按 snake_case 序列化并有排序；CapabilityHandle 公开 id/subject/scopes/expires_at_ms，可以序列化，但 authorize 查的是私有 broker map，不信任随手构造的 handle。CapabilityBroker 默认空 BTreeMap 和 AtomicU64 counter。

`issue` 验证 subject、scopes 非空、TTL>0 且≤24h；now_ms 和 Relaxed counter 组成 SHA256 输入：固定域 `zenpi-capability-v1\0`、subject、wall-clock 毫秒、serial，生成 cap_ 加 hex。scopes/TTL 不入 hash，没有随机盐或 broker/session 身份；不同 broker 在相同 subject/毫秒/serial 上可产生相同 ID，不能把 SHA 外观称为不可猜的 bearer 密钥。此处不做跨 broker 认证。expiry 使用饱和加法和毫秒截断；正 sub-ms TTL 变 0ms，counter 可 wrap，map 无容量上限且不自动清理过期项。

`authorize` 要现存 id、subject 完全相同、scope 存在，且 expires_at_ms **>=** 当前时间，等于 expiry 仍允许。wall-clock 回拨影响存活判断。`revoke` remove 并返回是否存在；`revoke_subject` retain 其它 subject 并返回删除数。`now_ms` 在 epoch 前 fallback 0，u128 毫秒 clamp 至 u64；不是单调时钟。broker 重建为空，不自动恢复/撤销真实子进程。本轮 src 调用搜索未发现它被生产扩展进程授权使用；实际 API 2 使用下节独立 ExtensionLease，不能混用两者的过期或随机性结论。

ExtensionError 完整分类为 Io/Toml（From 转换）、Invalid(String)、Path(PathBuf)、Incompatible{name,found,supported}、AlreadyInstalled(String)、Tool(String)。Tool 注册错误转成文字；runtime 又可转 InvalidDefinition。错误保留诊断，不说明此前目录、进程或 journal 没有变化。

## 6. 有界 runtime 上下文：lease、协商与 hook 结果（extension_runtime.rs 1–440）

本轮连续读取该相邻文件的生产前缀 1–744；其后测试模块不在本项全文范围。ExtensionLease 私有构造，identity=session_id/generation/capability，generation 是进程级 AtomicU64，capability 用两次 RandomState hasher finish 生成 ext_ 字符串；active Arc<AtomicBool> 被 clones 共享。序列化 identity 不可制造 live lease；check 只检查 active，**没有 legacy broker 的 TTL**。RandomState 不是这里明文承诺的密码学 token API，session/generation/撤销及精确响应关联才是代码可直接核对的边界。

ExtensionRuntime::prepare 先 load，再创建未 started 的 runtime/lease，按 enabled manifest.name 顺序对每个 API 2 调 initialize，要求严格 Negotiation 的 api_version=2、hooks 列表、tool name 列表与 manifest 顺序完全一致。API 1 不协商。每次用 manifest timeout，整个循环取消 closure 还检查 2s CHAIN_TIMEOUT；catalog load 发生在该 chain 计时之前，spawn/OS 调用也不被时钟抢占。失败候选 Drop 撤销 lease，尚未 started 时不发 session_close；已经执行的 initialize 外部效果不回滚。

chain 先 lease.check，逐已声明 kind 的扩展调用 hooks/call，严格解 HookResult，再 apply，再检查 event 序列化≤64KiB，结束检查 lease/cancel。输入原 event 没有统一先验 64KiB 检查；有 hook 时 request envelope cap 会先约束，若没有匹配 hook 则不经逐项 event 大小检查，不能说任意无 hook passthrough 也必定≤64KiB。2s closure 在请求循环检查，不能把 filesystem/syscall/JSON 工作视为硬实时。

| Hook | 允许结果与作用 | 失败传播 |
|---|---|---|
| input | Continue / Transform{text}，逐个更新 text，turn_id 不变；text≤64KiB、无 NUL | 无 handled/images；非法 action 或请求失败中止该 chain，局部变更不会返回给 owner |
| context | Continue / Context{instructions}，只修改 request-local instructions | 不接受 canonical/native/tool 历史替换；非法 action 中止 |
| before_tool | Continue / Rewrite{arguments object} / Deny{reason}；固定原 call id/name | deny 立刻返回 InvalidArguments，后续扩展不能覆盖；schema/path/policy 由 owner 再核对 |
| after_tool | Continue / Output{output}，最终 redact_json | runtime 可失败；真实 core 保存此前有效工具结果并报告 hook 错误 |
| session/agent lifecycle | 仅 Continue，按声明顺序逐项通知 | 每个错误记录到 Vec，继续尝试后续；总 2s 已耗尽时后续也会被取消，不能声称全部 callback 必然启动 |

start 先 started=true，重复 start 空结果；通知失败也不重试。close 先 closed=true、撤销旧 active，构造相同 identity 但独立 active 的私有 closing lease用于本次 close 通知，结束撤销；这个 lease 在一次 close 操作内可供多个扩展通知，不是全链只发一个进程请求。旧 tool/callback clone 此时已经失效。Drop 对 started 且未 closed 调 close("dispose")，忽略返回错误，然后 revoke；Drop 本身可能耗时，不是纯内存释放。

## 7. 请求、管道、取消/超时与清理（runtime 441–744）

process_request 先取消/lease check，API 2 无 lease 拒绝；API 1 固定 request id 1，API 2 使用进程级递增 request id、附完整 lease identity。先 serialize/bound，再加末尾 LF：tools/call payload≤1MiB，其它≤64KiB，故实际 request bytes 可比 limit 多一个 LF。stdin/stdout pipe、stderr null，cwd=extension.directory；env_clear 后固定 PATH=/usr/bin:/bin、LANG/LC_ALL=C.UTF-8，再加 ZENPI_EXTENSION_NAME 和 ZENPI_WORKSPACE。没有继承 provider 凭证，但 extension 可按 OS 权限访问文件/网络；workspace 只是环境信息，不是 cwd 或系统调用 confinement。

Unix exchange_frame 从 spawn 之前计时，owned child 和两 pipe 都在本函数；设置非阻塞后，每轮先取消/lease/deadline，再最多写 8192B、读 8192B（最多 limit+1），写完关闭 stdin，收到 EOF 才记 eof，try_wait 获取 child status。只有 stdin 已关闭、stdout EOF、child 成功退出同时成立才返回 frame；返回一行却保留 stdout 或不退出仍可能 timeout。整 stdout 是一个 JSON 文档，可有 JSON whitespace/LF，不是读到首个 LF 即接受，多个 JSON 文档会解析失败。response cap 含实际收到的 LF，request/response 边界不完全对称。

没有进展睡至多 5ms 且不超过剩余 timeout。取消、撤销、超时、write zero、IO、超 frame 都返回，OwnedChild Drop SIGKILL 整组，再 kill leader、wait，kill/wait 错误忽略。正常成功退出也经 Drop 清组；Unix setsid 离组后代可能仍活，host 不因其保留 fd 而 join 阻塞 reader。**此实现没有额外 pipe reader thread**，避免旧实现 read_until/join 阻塞；它不保证任意 OS spawn/wait/文件系统系统调用都在毫秒预算内返回，不保证回收全部离组后代。非 Unix exchange_frame 直接 Unsupported，不启动命令。

exchange 后再次取消/lease check，再 StrictValue 递归拒重复 object key（包含嵌套对象，防重复 action 擦掉 deny），number 拒非有限 float。严格 Reply 要 jsonrpc/id/result，可选 capability/error，拒未知 envelope 字段；jsonrpc必须2.0，id必须本次，API2 capability精确相等，error非 null 就拒。JSON null result 合法，missing result 由反序列化拒；error:null 被 Option 当 None，API1 可带能解析但不匹配校验的 capability。frame 上限前置并不代表所有解析/克隆峰值等于一个 frame。

错误在进程已执行之后也可能来自退出码、过期 lease、reply 身份、JSON 或 registry 序列化；不能默认重放。stderr 丢弃，CommandFailed 只提供状态等有界协议错误，不存在 stderr raw artifact 捕获。本文件移交 runtime 的协议与 run_command/raw output store 是不同通道。

## 8. 真实 core、TUI、headless 接线与发布边界

core prepare_extensions 只允许 open+Idle、有 host tools，拒 worker binding/BlueprintWorker origin；候选 runtime 协商后，clone registry、移除旧扩展工具名、注册新工具。configure_extensions 再取消检查，先 append extensions_selected，才 publish。publish 先 old.close/revoke，把 registry/runtime 切到 candidate，再 session_start；close/start 失败变脱敏 AgentEvent::Error，不回滚已经发布候选。set_tools 也 close("tools_replaced")。

configure_resources 先加载资源候选、准备扩展候选，检查取消、append provenance（含新 generation），再发布资源/扩展。resume 的 replacement governance/resources/model/extensions 全部准备在 commit 之前；commit(&mut replacement)? 成功后才替换 session/ports/output store/模型与资源、清旧事件，然后 publish_extensions("resume")。准备/commit 失败保持旧 runtime 活跃；新候选 initialize 已发生的进程外效果不会因此消失。try_close 先 runtime.close，随后 skill_session_close append 等可能失败，phase Closed 的设定在其后；因此 lease 已撤销而 Agent 返回错误是实际可达顺序。

AgentStart 在 complete_with_tools 前通知，AgentEnd 在结果后通知 complete/failed，后者用恒 false 外部取消函数但仍有内部预算。complete_with_tools 对 worker+hook 再拒绝；input hook 读取有效本轮文本，模板可能用 active_resource_inputs，InputPump poll 连接宿主取消/输入，pump.finish 失败也先于结果传播。input transform 只替换 provider prepared turn，不改已存原始用户 turn。context 每轮构造 skill/persona/resource instructions 后运行，再算 instruction token budget；只影响该 request，canonical 历史不向插件开放写权限。

有 before/after hook 就把 tool batch 并发降为 1。before_tool 先检查原 skill/effect/approval deny，再 validate_hook_call 原参数→hook→validate 改后参数→pump.finish；变化写脱敏 extension_tool_rewrite，随后 prepare_tool 对真正参数执行审批/准入。失败时 owner 返回 Denied 或 Cancelled，待请求的模型工具不会 dispatch；此前 hook 程序可能已启动。after_tool 只处理成功工具结果，失败或再次 compact 失败记 extension error，保留原有效 invocation。stdio/JSON、结果脱敏、持久化不应被误读为工具外部效果事务。

入口 main→core::run。extension CLI 分支直接调用 ConfigPaths.extensions 的 load/install/upgrade/remove/set_disabled，输出 summary/changed；正常 Agent 分支先创建 tools/resources，配置审批 TUI=ReadOnly、Headless=Always，再 configure_extensions，之后交给 headless::run_stdio_owned 或 tui::run_async_with_profile。项目 prepare_project_with_options 也建立 tools/resources 并 configure_extensions。CLI 命令退出与正在运行的其它 Agent 不共享一份内存 lease，磁盘管理不自动热更新它们。

TUI 11668–11766 把 Agent 放入 Arc<Mutex>，BackgroundRunner 在 request owner 上调用 process_with_cancel_and_events，token.is_cancelled 传入；ResourceControl 调相同资源控制入口。13315–13370 的正常清理先恢复 terminal，再 shutdown_and_join 各 runner、close_all owners。未全文读取 pool.close_all，不把局部调用推广为所有 owner cleanup 必成功。headless 774–788→run_async_stdio；4272–4292 恢复 owner pool/replay，4333–4508 runner 核对 expected_session、submit/run_active 或资源控制，传同一 cancellation token，持有 owner lock 到事件归入 request，再 mark_completed。4770–4815 cleanup 对可锁 owner 调 try_close，繁忙 owner 进入单个 deferred cleanup thread；进程可能在回调前退出，不能承诺每个 close hook 必送达。这里只核实调用链，不验收 TUI/headless 全文件或借用其历史测试。

## 9. pi 上游对照：只映射已读语义

本轮读 pi runner.ts 927–1070、1246–1286。emitToolResult 依次合成 content/details/isError/usage，异常 emitError 后继续；emitToolCall handler 返回 block 便立即停止，此段没有相同 catch。emitUserBash 可提供替代结果；zenpi 当前八种 HookKind 没有 user_bash。emitContext structuredClone messages 并允许 handler 替换整个 messages；zenpi 仅 request-local instructions，保护 canonical/history 的权限更窄。emitInput 可 handled、transform text/images，携带 source/streamingBehavior，异常隔离继续；zenpi 只有 text continue/transform，错误中止当前 chain。

上游是 in-process async handler/ctx，当前目标是每请求独立 executable/JSON strict reply/lease；不宣称任意源扩展可直接安装，也不声称生命周期名称、异常恢复或图像语义完全相同。所读上游函数内没有 zenpi 的固定 2s/1s 监督协议；未读 helper 或其它 owner 的安全语义不外推。

## 10. 测试断言、历史成功与保留失败

tests/extensions.rs 自身 5 个普通测试、0 ignored；4 个 Unix 条件测试和 1 个跨平台 broker 测试。fixture write_extension 写 API1 shell，脚本读取一行、检测 env 中 OPENAI 或 ZENPI_API_KEY、打印固定 reply；无参数 echo 断言。本轮源码及 fixture 全读，**全部未运行**。

| 测试 | 实际断言 | 不冒领的覆盖 |
|---|---|---|
| local_mcp_tool_is_framed_and_child_environment_has_no_provider_auth | catalog 一项、registry 调用 Success、输出 ok:true 且不含 OPENAI；fixture env 检测失败会 exit90 | 没注入每一种敏感变量；名称不是完整 MCP 互操作证明 |
| extension_lifecycle_rejects_incompatible_and_never_loads_disabled | install→disable 无注册工具→enable→upgrade version2→remove true/false；API99 install拒且无future目录 | 不覆盖 post-rename load 失败、rollback失败或 live lease 磁盘更新 |
| manifest_path_escape_and_excess_permission_fail_closed | 只把 executable 改为 ../server.sh，load失败 | 名称包含 excess_permission，函数没有该断言 |
| capability_handles_are_scoped_opaque_and_revocable | cap_ 前缀、不含 key、subject/scope拒绝、revoke后失效 | 未测密码学随机性、TTL边界、revoke_subject 或跨broker |
| extension_cli_installs_lists_disables_enables_and_removes | 真实 binary 的 install/list JSON/disable/enable/remove 成功 | fixture 改 child HOME 并移除 ZENPI_HOME；历史 launcher 的 HOME_preserved 不适用于全部子进程。本轮未重跑 |

完整 prior/target080-review-3.1.20-ready 共 19 files，prior/extension-resume-startup-audit-ready 共 72 files。前者的旧报告 38013B 原样保留，含更早报告前缀；4 个 archive 保存 42+34+21+6=103 members（最后6是历史 context，不计新阅读）。后者保留 216-input archive、native/awk rejected patches、每次失败、成功和诊断；全部为历史数据，不执行其中脚本或二进制。

本轮完整读旧报告、旧 README、startup index/recommendation，及明确记录在 ledger 的原始日志。旧080 G-STAGE 记录 structural=true 但缺 master receipt 因而 ok=false，不能改绿。原115 acceptance-final.log 是**历史60 passed**（8 approval+5 extensions+17 hooks+30 tools）。pipe prescribed-native-final.log 是**历史116 passed / 0 failed / 3 ignored**（56 library+同一60 integration）；两组重叠，不加成176，12 native场景也不当12顶层测试。更早 prescribed-final-bounded-concurrency.log 为55 library passed/1 failed/1 ignored，API1 stdout 的 fixture never escaped；该失败与 final pass 同时保留。

startup audit 的原 B isolated log 在 configure_extensions 阶段 CommandTimeout(1000) 失败；本地原 fixture 历史成功。native 历史先成功后失败，awk 四次成功后第五次失败，原始 stdout/stderr 与 rejected patch 均保留。它们不能证明可靠修复，不能用通过次数掩盖反例，也不证明 resume commit/lease 的后续断言本身错误；具体 OS 延迟机制仍未证明。本轮不启动 Python/native/awk fixture、不预热、不改 1000ms，不重跑 117/131 或其它产品。

## 11. 函数映射与未运行验收条件

definition-index 提供实际声明行，semantic-map 按 source 1–626 连续范围关联上述章节和消费者，test-source-index 将 5 个测试定位到实际函数；索引是可复核导航，不是全文阅读证据的替代。reading-ledger 同时记录 3 段 source、32 段 context、历史文档/归档内日志的实际身份；相邻文件只作上下文，没有增加 final owned artifact。

后续若主控另行授权产品验证，应分别核实：清单 args/tools/hooks/count/UTF8 边界与 disabled 例外；目录名和 manifest.name 不一致时 summary 选择；晚注册冲突不变原 registry；API1 无 lease、API2 exact negotiation/id/revoke；hook 整体 envelope 大小与无 hook passthrough；原参数与 rewrite 后 schema/path/审批；initialization 已执行而候选 commit 失败的外部状态；install 发布失败、upgrade恢复失败、固定 temp/嵌套 copy 与真实目录残留；磁盘 disable/remove 对旧 runtime 的区别；broker TTL sub-ms/equality/clock rollback/revoke_subject/cross-instance；pipe超限、child早退出但持fd、取消与离组后代；close 回调失败、deferred cleanup 进程退出。以上均是**未执行条件**，不是本轮 pass/fail 或已复现漏洞。

本包至多一次新纯 Python 标准库离线结构审计，冻结后全文静态读 auditor/wrapper，再执行；日志、guard、receipt在ready外。审计仅验证身份、读取区间、源到报告映射、单路径正反patch、历史档案、001依赖及受保护旧包/94 non-.ops文件，不执行产品，不把结构通过写成语义接受。原077包与全部旧证据不改；交付后停止本项，下一项须另行指派。

# ZS1-080 主控独立全文理解验收 — 3.1.21

接受范围仅为 `src/extensions.rs` 的逐文件完整理解及行为映射。主控逐行读完当前 20742B/626 行、冻结基线 23071B/685 行、候选报告 153 行和 `tests/extensions.rs` 6302B/180 行。当前源码 SHA256 `38a242eff3ec911f4560e3dea3868b742d511205ff0ccab1b622962b41e420ed`，冻结基线 `9b1879edcfb70e711960092e12060a9137e2aacda6bdc547318e5ccb7510fe39`。同一文件的基线与当前分别阅读，没有通过分区、摘要或函数索引替代全文。

主控还读了当前 `extension_runtime.rs` 1–744 的生产前缀，以及 core 的准备/发布/资源 reload/turn hook/审批 rewrite/after-tool/resume/关闭/CLI 分派、真实 TUI runner 和终端恢复、headless owner 调度及 deferred cleanup 的有界片段；上游 runner.ts 927–1070、1246–1286 用于对照。相邻文件未在此项获得全文验收。来源、区间、工具输出与当前字节绑定见 read-binding.json。153 行 worker 报告完整保留，主控意见另附，不改写其旧 53/121 捕获事实。

## 数据、加载与注册

逐一核对 manifest/permissions/tool/summary/catalog、两个 API 版本及 hooks 默认值。默认 tools/hooks 为空但启用校验要求至少一种声明；hook timeout 即使无 hook 也须 1..1000ms。ID 允许 ASCII 大小写字母数字、下划线、横线；version 只是字符集合，并非 semver 或升级排序。工具定义再通过共享 ToolDefinition 校验，不能将第一层 128B ID 上限当成全部约束。

load 直接目录排序、entry 读取错误被 filter_map 丢弃、点目录跳过、symlink child 跳过、manifest symlink 拒绝，enabled executable 最终须在 canonical directory 内且为文件。内部可执行 symlink 并非一律禁止；没有固定 inode/内容摘要或执行权限位。原文的“本地扩展”不能解读为 OS 沙箱。

当前 load 使用 take(256KiB+1)，而 install/upgrade/set_disabled 仍先完整 read_to_string；只有 enabled catalog 有 32 项上限，disabled summary 和目录枚举没有同等总量约束。disabled 项通过 TOML 反序列化后绕过 validate、重复名与执行路径校验；compatible 仅指 API 数字。读取和使用不是单个文件系统原子操作。

基线直接逐项写 registry，当前 clone candidate 后整体替换，晚冲突不留下部分注册。API2 的普通 register_tools 拒绝后必须经 session runtime/lease 注册。LocalProcessTool 默认 Sequential；invoke_cancellable 再查声明 permission，统一交给 process_request，tools/call 固定 30s，不能套用 hook_timeout_ms。network 标记没有对应 OS 网络隔离。普通 execute_inner 有 call/policy/gate/builtin 准入及结果脱敏/尺寸检查，不运行完整 JSON Schema；hook rewrite 才有专用严格 schema 子集。

## 磁盘生命周期与 capability

完整核对 install/remove/upgrade/set_disabled/copy_tree 正常与错误顺序。install 在 rename 后 catalog load 失败可留下目标目录；upgrade 备份恢复为 best effort，删除 backup 失败发生在新目录发布之后；固定 tmp、无 fsync/锁/通用 cleanup，不构成持久事务。copy_tree 遇 symlink 仅清当前递归目录，其它失败可留外层 staging；没有总字节、深度或取消预算。source 与 root 嵌套、目录名与 manifest.name 不同以及 disabled 同名 summary 的 `.find` 选择等是明确的待验证边界，本轮未运行构造场景。

磁盘 disable/remove 与已载入 registry/lease 分离，不自动广播给其它进程。manifest 校验不保证后续复制和执行字节不变；可执行程序一旦运行，host 后续返回 Err 不说明其外部效果未发生，不能自动重放。

legacy CapabilityBroker 的 SHA 输入是固定域、subject、墙钟毫秒及局部 counter，没有随机盐、scope/TTL 或 broker identity；不以 cap_ 外观背书不可猜性。authorize 使用私有 map、subject/scope 及 expires_at_ms >= now；sub-ms TTL 截断、墙钟回拨、跨实例同输入/清理容量等均保留。该 broker 与生产 API2 ExtensionLease 不是同一授权机制。

## runtime 与真实入口

runtime 的 session/generation/identity 和 active Arc 撤销按当前源码复核；prepare 的 initialize 先于 owner publish，候选失败会撤销 lease，但不能回滚已发生的扩展进程效果。严格 negotiation 要 API/hooks/tool names 顺序精确相等。链预算 2s、单 hook 1..1000ms、tools frame 1MiB/hook frame 64KiB 分层，实际系统调用/解析不因此成为硬实时。无匹配 hook 的 passthrough 不执行逐项 event cap。

Unix supervisor 持有非阻塞 stdin/stdout，分段写读，等待 stdin 关闭、stdout EOF 与 child success；不是“读第一行即成功”。成功和失败都走 OwnedChild Drop 清组、kill/wait；离组后代和不可抢占 OS 调用不受其绝对回收保证。非 Unix 返回 Unsupported。StrictValue 递归拒重复 key，Reply 校验 jsonrpc/id/API2 identity/error，stderr 丢弃。旧 reader thread/read_until/join、宽松回复校验代码仅在基线内，未执行。

core 的 old runtime 在候选准备/commit 前保持有效；provenance append 后 publish 先关闭旧代再替换并 start，通知错误转事件、不回滚已发布候选。before-tool 对原参数与 rewrite 后参数重新校验，再按真正参数审批；after-tool 失败保留原有效结果。try_close 的 lease 撤销早于后续可失败 journal write，完整 close 则兜底 phase Closed。TUI 的正常退出先恢复终端后 join 和关闭 owner；headless 忙 owner 可延迟清理，进程提前退出不能保证 close hook 送达。这里只核对静态入口，不增加新的 PTY/HTTP 或生命周期运行验证。

pi 对照明确保留差距：上游 in-process handler 可处理 text/images/handled、user_bash 和完整 context messages；目标八种 subprocess hooks 仅允许指定结果，context 只作用于本次 instructions。未宣称任意 pi 扩展可直接安装或达到 1:1 完整扩展兼容；此差距属于产品 115 等后续验收。

## 证据与结论

主控新建只读 auditor 副本，唯一修改为显式不可变包路径和 manifest SHA 固定；完整静读输出 43ff9d 后只运行一次，cf07d4 结果 **39 个结构检查通过、0 失败、stderr 0B、产品执行 0 次**。检查覆盖 208 payload、连续读取身份、测试索引、单报告正反 patch、19+72 历史文件、103 历史归档成员、216 历史构建输入、001 依赖 artifact 及受保护旧包。原 worker auditor、旧测试/探针/管线均未重跑。

现有测试 5 个：4 个 Unix、1 个跨平台 broker。名字包含 excess_permission 的测试实际仅验证 `../server.sh` 路径拒绝；opaque 测试不证明密码学属性或 TTL。CLI fixture 会改其 child HOME，本轮只阅读没有运行，不能借父进程未改 HOME 扩大覆盖。历史 60 与 116 Rust 结果重叠，不相加；旧 G-STAGE 缺 master receipt、fixture never escaped、native/awk 启动反复超时与 rejected patches 全保留，不被本次结构检查消除。

结论：仅接受 ZS1-080 单文件理解。产品 115、117、TUI 生命周期与安装错误恢复等整体门禁继续未完成；不把当前文件验收当成这些边界已修复或已运行通过。主库生产源码、测试、BentoBox 与顶部加号路径绑定在本次接受中均未更改。

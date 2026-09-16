# ZS1-130 外部编辑器合入复验 — 主控 3.1.19

已合入七条 owned 路径：src/config.rs、src/external_editor.rs、src/lib.rs、src/tui.rs、tests/config.rs、tests/tui_composer.rs、tools/tui_external_editor_smoke.py。Ctrl-G 将完整草稿交给前台编辑器，保存退出仅回填仍有效的原项目草稿；明确再按 Enter 才提交。保留 BentoBox、项目路径绑定、现有输入队列和同步 /input 路由。

冻结 worker manifest 067fcead5c8c65df8c12814f25feafbf896ae6c633947799a1d6cbd9c4e0fa32 的272个artifact逐个验证，原七路径patch在私有Git中正反向恢复。主控逐文件读取完整新增模块745行、配置/两类host/测试差异和原1094行PTY工具。src/tui.rs 唯一合并冲突来自较新的A同步输入路由；保留run_with_state_controls、Agent实际input owner和普通callback不接收control事件的三处原有差异。merged-vs-worker.diff及原integrated.patch保留，不用worker整文件覆盖主控。

## 实际行为证据

主控当前release SHA256为7a374a3b0764c7a4db9b3b53e36dc07d3d8b17b80f5711805a932a2090e60774，6000768 bytes。独立固定before二进制对Ctrl-G不启动编辑器、0 HTTP、原草稿保持作证。当前异步生产host包含原有101/104/111/129/131整合。

完整外部编辑器入口六组通过：roundtrip3、files/cancel/prefetch22、busy2、lifecycle6、configuration26、sync1，共60项行为检查。原JSON有62个checks字段，其中两个是清理失败后保留路径的诊断值，不计作独立检查；configuration的near-4MiB条目是已执行断言后的数据记录，计一项。未修改原断言来修正统计。保存/退出后的残留CR没有提交；一次后续明确Enter发送完整Unicode/LF/空格正文。真实child记录包括PID/PGID/foreground、stdio TTY、cooked/echo/blocking、0700/0600和完整seed哈希；审计42条started记录（包含原和纠正后的sync实例）。生命周期组未检测HTTP，不泛化其网络断言。

复核发现原独立sync consumer没有继承仓库Cargo.lock，即使offline也选中22个不同缓存依赖版本。保留原成功证据，限定其为兼容版本consumer测试；仅修正synchronous_host，在构建前复制当前锁文件并使用offline构建，记录源/consumer锁哈希。新的实际PTY同步回填、保留尾LF、残留CR无提交、随后明确Enter回调通过，逐包name/version/source/checksum与主控一致。helper其它函数AST不变。其产物和锁文件独立留存，未更改生产Cargo锁。

旧TUI原脚本顺序通过122项：输入15、kill/yank29、queued7、large14、burst7、project12、menu24、approval14。同一固定release验证顶部鼠标＋打开目录选择器、取消/非法路径不造占位、Unicode/空格路径、同名目录、两个实际shell各自cwd、项目配置实际backend model、后台结果归原项目、重启和草稿恢复。保留BentoBox。另独立Agent-backed同步 /input 真实两个PTY进程通过enqueue/edit/cancel/list和持久恢复四组，取消/编辑不发HTTP，明确提交后一次HTTP含原pending ID。

Rust集成回归通过106个顶层测试：config15、layout7、input host23、BentoBox16、composer45；两个既有helper ignored。全targets/features严格Clippy、格式检查通过。所有142个记录的源/vendor/Cargo/测试/工具输入复验；唯新增同步工具修正有明确前后记录。结构checker通过，不等同语义/全部验收通过。

## 尚未通过的门禁

规定包含lib的组合命令失败：79 pass、5 fail、5 ignored；一次serial验证83 pass、1 fail、5 ignored。剩余escaped_pipe_cancel_is_bounded的api1 stdout/stdin在native入口和escaped-ready记录出现前耗尽1000ms。并行core resume失败发生于configure_extensions initialize，尚未执行resume commit断言。不能只归因为并发，不能把未达到endpoint前置条件当成已验证管道清理。原失败日志全部保留，B正在补齐outer watchdog杀helper时丢失的phase证据，不提高任何deadline。

当前实际budget五个冷启动样本为1208.732708、35.488291、33.633667、37.030875、35.111416 ms；首样本超过1000ms，budget命令exit1，整体门禁未通过，不删掉首样本、不预热后冒充通过。其余门禁通过：15直接依赖；6000768 bytes；RSS max5226496 bytes；queue1456.5645us、render693.566us、layout0.833733us、10000脏请求合并为1帧。尚未定位冷启动超时具体原因，记录为需继续诊断的真实失败。

本报告确认定向整合与实际交互证据，不接受整个130，不接受任何尚未闭合的依赖。平台为macOS ARM；Linux保留Unix实现和可运行工具但未实测，non-Unix不可用。PGID只收束前台client及同组后代；setsid服务由夹具显式清理，不宣称被产品自动回收。临时文件容量轮询不是文件系统写配额。

回滚仅使用七路径定向差异及后续helper锁修正的反向差异，保留主控原有A路由、有效项目/会话/草稿；不reset/stash/clean。所有成功和失败门禁同列于audit-result.json和manifest.json。

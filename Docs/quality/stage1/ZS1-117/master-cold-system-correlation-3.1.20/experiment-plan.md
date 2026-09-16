# 最小后续实验方案 — 提交主控审定，尚未实施

本次历史首样不能再变成“从未运行过”的同一产物。对已有e9ba...反复运行、复制旧文件再运行、删provenance/清缓存、重新签名或换time包装，均不能还原同一历史首样，不推荐用来得出修复结论。当前证据已足够拒绝“诊断JSON写盘导致秒级差异”解释，并将同路径系统扫描列为领先假设；要进一步分辨main前等待与产品阶段，需要下一次明确批准的全新诊断产物。

建议只做一个独立诊断构建的一次首启观测，预先固定下面设计；不运行本方案直到主控授权。正式release预算仍保留原脚本、3样本、1000ms、完整timing边界，且必须是该正式产物未经预热的正常下一次发布验证。诊断产物有独立源码/二进制SHA和路径，其结果绝不替换本次gate，也不宣称能反证历史1109.425125ms原因。

1. 在独立私有源码副本加一个仅诊断构建启用的极小phase emitter，编译一次，归档全部输入、工具链与产物SHA。发布源码不改。诊断标记采用预先继承的匿名pipe FD（不是磁盘FIFO、不是PTY），每条定长小于PIPE_BUF，最多5条；记录phase、PID、PPID、mach_absolute_time原始ticks，父同步保存boot UUID与timebase。禁止读配置/日志文件、格式化大JSON、锁全局日志器或fsync来打标。
2. 在Rust main第一条语句标M；core::run即将进入headless transport前标H；headless异步owner/replay/reader/runner就绪、进入接收循环前标R；shutdown drained response完成flush后标A；main返回ExitCode前标X。这5个点已足以分开[M,H]产品初始化、[H,R]host启动、[R,A]shutdown协议、[A,X]回收，以及parent-launch→M的main前区间。先不大规模给每个dependency打点；若某段明显大，再另提有依据的owner内定位，不一次铺开所有模块。
3. 新私有观察器完整读审后，只一次启动该新诊断产物，仍通过同一`/usr/bin/time -l`、相同headless/session/payload/env覆盖、cwd和新临时目录。父在Popen前/后、首次完整stdout JSON可观察时、EOF/wait完成时同步记录mach ticks；并保留原完整outer perf_counter elapsed。trace reader不得阻塞产品（总输出<=数百B、独立drain），stdout/stderr/trace全部原始字节、command、target PID/PPID、wrapper PID、inode/mtime、hash、fixture及journal在失败时也保全；观察器必须5s固定timeout、kill整个独立process-group并bounded reap，不复用旧wrapper-only observer。
4. 只读系统查询严格限定那次新启动窗口和唯一artifact路径/已见path token/标识，记录scan/result/exec allowed原始事件；不做特权DTrace重试、不预调用spctl评估、不codesign verify/修改、不预运行新二进制。签名display/hash均记录具体观察发生在启动前或后，不假装它们完全无缓存影响。
5. 预先固定判读：若主要延迟发生在parent→M且同路径扫描覆盖该段，M→X很短，则进一步支持main前系统首次执行等待；若M→H很长，则定位在产品初始化；若H→R、R→A、A→X很长，则分别指向transport初始化/关闭协议/回收；若X之后或Popen开销很长则优先wrapper/EOF/宿主调度。标记缺失、时钟域无法配对、time或子进程未回收一律保留为不确定，不自动重跑至绿。

认识边界：即使M晚到，parent→M仍包含time wrapper到target exec、loader、Rust main前runtime，单靠5点不直接分离每个OS环节。要确证某targetPID在具体内核策略等待中阻塞全段，需要受支持且经授权的PID/exec/wait tracing；当前没有这一能力证据，不预设可用或请求绕开已拒权限。上述一次非特权诊断可先区分主控要求的产品初始化与main前/退出包装大类，并用同clock真实PID强化同路径扫描关联，成本和代码改动最小。

验收本实验只要求证据完备及分类能力，不要求时间低于1000ms。即使新诊断首样快，也只能说明该产物这一轮没有重现长阶段，不能归因历史故障、更不能把快样替换原失败。

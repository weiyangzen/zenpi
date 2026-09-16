# ZS1-027 — find.ts

状态：[_] worker 完整阅读候选，主控未接受。
source_id: SRC-0949
source_path: packages/coding-agent/src/core/tools/find.ts
source_hash: b06bcae6821a0e9fda1b63be613a7ce28eb0e66e1d01d16564e59e83f9ce10ea
source_bytes: 11178
read_ranges: [[0, 11178]]
run_id: zenpi-stage1-20260911

完整阅读 1–318 行（1–300、301–318）。schema为glob pattern/path/limit默认1000。relativizeFindResultPath使用指定平台path模块把绝对结果相对searchPath并统一POSIX分隔符，保留尾目录分隔符；它本身不验证结果是否逃出root。defaultFindOperations.glob是占位[]；真实default分支执行fd，不能拿占位分支当生产能力。

customOps.glob分支先exists、前后abort检查，传ignore node_modules/.git与limit，返回结果统一相对化并按bytecap truncateHead，>=limit即resultLimitReached。custom返回的绝对路径可被相对化成..而没有workspace拒绝合同，zenpi不能直接复用该宽松假设。

default调用ensureTool(fd)，可能下载；本次zenpi机器只有rg15.2，没有fd，候选不下载、不假装存在fd，而用真实rg文件枚举+glob过滤提供明确engine= ripgrep。原fd参数--glob/color=never/hidden，查searchPath祖先.git；仅非gitrepo加no-require-git，避免跨nestedrepo边界继承父.gitignore（代码引用issue5960）。max-results使用effectiveLimit；含/的pattern启用full-path，并为普通相对pattern加**/以匹配绝对路径；Windows替换分隔符字符类。

fd stdout逐行收集到数组；stderr无单独cap；close后把行trim（因此首尾空格文件名会变化），规范CR、统一相对化、截总bytes、附resultLimit标志。非零exit且已有stdout会继续返回部分结果；只有无output才reject，zenpi候选选择保守错误，不把执行失败包装成成功完整集合。AbortSignal早期和exec中都会settle，kill child后Promise可在回收完成前拒绝；zenpi候选必须由SupervisedChild Drop/cleanup完成group kill+wait后返回。

112保留tool=find的显式glob入口、workspace相对路径、ignore/hidden选择；使用NUL分隔保留空格、控制路径长度并拒绝换行/NUL/../外部canonical目标及symlink结果。限制文件数、深度、输出、时间和累计child-start预算，缺已安装rg返回Unsupported；默认search_text literal不受缺rg影响。不复刻fd全部平台/目录结果模式，报告不能宣称fd等价全量能力、worker advanced已获授权、或已完成主控host预算接线。

# ZS1-117 — 当前自然新 release 的预算检查（3.1.20）

本次门禁 **FAIL**：首次启动1044.823959ms超出未修改的1000ms上限。保留全部五个样本：1044.823959、42.234125、44.452083、42.458084、45.956333ms。中位数44.452083ms不能替代max判定；没有重复跑门禁取通过值。原1208.732708ms失败也保留，不宣称已修复。

运行当前原工具 `python3 tools/bench_runtime.py --samples 5 --output .ops/stage1_execution/release-current-20260912/budget.json`，由工具自然构建新release后立即测量。事前未执行此新binary的--help/预热。它包括前次新增/new跨项目控制worker和全部当前源，225个构建输入已在命令前冻结并在命令后逐个hash核对无漂移。固定实际release SHA256 `180ec9f70d5bedda5ca3b417a88bf313fefa069df8cf3c4488c7669133e149bf`，6032944B；私有目录保留可执行原字节。本记录不把后续debug单文件测试当成该release的功能证据。

8项预算中7项通过：normal dependencies、release字节数、启动RSS、queue round trip、render、layout和render coalescing。启动RSS最大5144576B；布局1.0733167us/操作，渲染654.478us/操作，队列1576.8345us/往返，10000次dirty请求保持1帧。它们是原工具定义的回归样本，不是p95或真实TUI交互延迟上限。

工具保持原来fresh temporary ZENPI_HOME/五个session、headless单shutdown请求、time(1)包装及相同cwd/环境覆盖的测量范围；原HOME和CODEX_HOME未改。构建和runtime_probe不计入五个startup时间。未改阈值、未预先创建其fresh home、未做签名/隔离/cache调整，没有从失败时间中扣除系统开销。

现有bench成功样本仍只保存elapsed/RSS、丢弃每个子进程原始stdout/stderr和PID，不足以定位1044ms发生于pre-main、产品初始化还是退出。因此本记录只证明当前门禁失败，不能将其归因于macOS策略或任一产品函数。旧诊断虽有路径关联的系统策略区间，也不能绑定本次样本。下一次自然新构建前应先改进独立记录手段，在原计时终点之后保存raw streams/阶段证据；后续已执行binary的诊断不得冒充首次冷启动。

ZS1-117、最终release及Stage 1仍未验收；这份失败不会阻止其它已授权的实现和逐文件/目录主控工作。

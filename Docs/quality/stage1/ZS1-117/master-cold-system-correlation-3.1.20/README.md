# ZS1-117 冷首样只读诊断 ready

原budget实际exit1：1109.425125ms > 原1000ms，后两42.552459/42.5525ms；固定3样本/7其他gate通过，未重跑替换。实测二进制6072608B，SHA256 e9ba9476f7699d715ad3b0b5fe4a07aeee9f280508d3cb495d59cfa94fff6449。

诊断见diagnosis.md：实际首样窗口内同路径/签名标识的syspolicyd扫描到结果1044.2375ms，6条原始事件、同boot/mach125:3重算。系统扫描等待成为领先解释；无目标PID/exec/main历史标记，完整根因仍未证实，117未修复/未接受。外层包装额外0.414625ms，JSON/hash落盘在timer外；不减去OS区间、不用热样覆盖失败。

5份完整源码/benchmark/helper/Cargo阅读及20份有界依赖，原始样本/流/log/run/历史失败、224构建输入当前比对（仅Gantt漂移）、10份只读系统命令实际argv/时间/exit/rawhash、metadata/签名display均保存。原始host ps只保存全hash与限定名单摘要，避免无关进程披露。3170旧ready文件（含083）hash保全。

experiment-plan.md是待主控审定的单个全新诊断产物首启/5phase最小方案，尚未构建或运行。没有product patch，没有G-STAGE/主控receipt或完成状态写入。新zenpi/Cargo/Node/PTY/HTTP/network/FIFO/测试/旧观察器/旧预算执行全部0，无新task/subagent。

verify.py只核对封闭artifact hash/原始样本与log/连续范围/系统事件算术。实际回执输出到包外zs1-117-release-cold-diagnosis-work/offline-verification.json；离线通过不代表根因证实。导航缺scripts目录和缺.cargo/config.toml的真实错误/退出资格保留，未创建文件掩盖。

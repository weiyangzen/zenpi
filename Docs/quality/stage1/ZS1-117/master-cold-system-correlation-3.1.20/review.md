# ZS1-117 — 冷首样系统扫描关联复核

当前正式预算仍失败：固定三样本 1109.425125 / 42.552459 / 42.5525 ms，原 1000 ms 门限不变，其余七项通过。这里增加原因定位证据，未改代码或重新运行预算，117 保持未验收。

主控独立读两份完整精确时间窗原始系统日志、实际命令回执、签名显示、boot UUID 与时钟比例，确认六条去重事件。scan 开始与 evaluate 相差 25,061,700 mach ticks，按 125/3 换算为 1044.2375 ms。XProtect 结果和 kernel exec allowed 明确指向本次 target/release/zenpi；路径 token 连接扫描起点与结果，结果标识与事后签名显示一致。该区间落在原首次启动的父进程 UTC 窗口内。

这足以把进入程序前的系统首次执行等待列为领先解释；尚无历史目标子 PID、exec/wait/main 标记，不能把完整 1109 ms 都判给系统，也不能从预算中扣除此区间。系统扫描自身为何较慢仍未定位。

主控完整读当前 main.rs 11 行、bench_runtime.py 338 行，与归档字节一致。timer 停止后才做 base64/hash/样本落盘，父 Popen 约 2.026 ms，外层差值约 0.415 ms，因此这两处不能解释秒级差异。communicate 仍包含 time wrapper、目标 exec、初始化、shutdown 和 EOF；产品内等待尚未排除。worker 的其余源码范围保全、核验 hash，本次不把它们算作主控全文件/目录验收。

107 个 worker artifact 全量复制冻结并离线验证退出 0。主控另核对原 budget log/run/summary/三个样本/二进制/构建输入清单与主库实测文件逐字节相同，复算六事件时间。离线通过只证明材料一致性与算术，不证明因果。早期超限和后来通过的不同产物历史均保留，不合并成当前样本。

已派原任务 B 按附带方案执行一个全新私有诊断产物的一次首启：匿名 pipe 的 M/H/R/A/X 标记，同 boot mach 时钟、目标 PID、完整原始流和 5 秒进程组清理。正式源码、旧失败、3 样本和门限保持原样。新诊断即使较快也不替代旧失败。

- [worker 诊断全文](diagnosis.md)
- [一次首启诊断方案](experiment-plan.md)
- [主控核验](master-verification.json)
- [离线核验](offline-verification.json)
- [原始完整 worker 证据包](worker-evidence.tar.gz)
- [原正式失败](../master-diff-budget-3.1.20/review.md)

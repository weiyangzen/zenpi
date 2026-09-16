# ZS1-098 主控独立验收

主控独立核对当前 Cargo.lock：46021B/1802L，SHA `7311d1133e06e428707d9aa10c95c6a0c1efa983865472f6b5e24b640f4b1348`。逐段读取其版本头、全部 196 个 package 块、source/checksum、依赖数组、平台包与末尾 EOF；结构解析确认 196 条记录、191 个不同包名，registry 条目均为 64 位小写 checksum，只有本地 patch 的 crossterm 与根 zenpi 没有 source/checksum。报告中的 196 包 ledger、389 条引用、5 组同名多版本、Cargo.toml/vendor 消费者边界与冻结 baseline 差异均已复核。

Cargo.lock 不是运行时代码；本次不宣称 Cargo 构建、下载 checksum、跨平台编译、vendor 安全或 feature 激活通过。worker 的 859 项静态校验、0 失败、0 产品执行和原始 capture 失败全部保留。只接受 ZS1-098 单文件理解，不改变 Cargo.lock 或 TUI。

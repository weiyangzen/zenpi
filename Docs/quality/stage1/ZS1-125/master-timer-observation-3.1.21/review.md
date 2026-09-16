# ZS1-125 审批计时组件反例 — 3.1.21

本次不接受125，也没有产品修复。当前主库TUI639010 B/b15661a160a855ef974e82dfb40a63c8d0c5777623cd553d1d4cf2069d8b59a1，主控独立新建public-state Rust观察程序，使用真正TuiState方法和TestBackend读取BentoBox第二行计时。仅创建验证过的ApprovalRequest投影，不运行coordinator、模型、工具、实际PTY或headless；HTTP请求0。

4个不同TuiState按1150ms观察窗口各执行一次，实际结果：

- 待批1项时set_status("Draft kept")后0→1s；期待仍暂停，失败。
- retire最后1项后pending0但0→0s；期待无需等下一个ToolStarted便恢复，失败。
- 先present/dismiss审批再set_busy(true)，pending1却0→1s；期待暂停，失败。
- 2项待批只retire1项，pending1且0→0s；作为继续等待控制，通过。

源读取解释是set_status把Approval子串当暂停条件、其它字符串变化会恢复；set_busy(true)总清零起计，present/retire不驱动计时。观察是公开组件动态反例，不能推导真实provider已停止/允许副作用、所有事件序列或精确1150ms预算。1150ms不是产品阈值、性能样本或复试策略。worker B已转125修复，要求确定性时钟回归、真实owner/取消/项目/多个审批边界，不能只替换成任意单bool。

唯一private Cargo manifest依赖主库path，原生ARM/offline构建。首次私有包构建更新其自身Cargo.lock，主库Cargo和全部源/vendor/tests输入前后身份未变；没有修改HOME/用户配置。before-observation真实exit1，3失败1通过；stdout保存4组完整header，stderr与命令时间记录原样保留。此处不执行旧runner、不做release/预算/117/131探测。二进制与代码/私有锁/日志归档，不能把本程序当zenpi生产二进制。

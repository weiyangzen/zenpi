ZS1-304 pending_thread_approvals.rs 完整单文件 learn 候选；主控 G-FILE 待审。

master.patch 只新增标准报告，主库已逐项只读确认基底不存在；worker.patch 替换原1891字节报告。每个 delta 在独立 git init 临时仓库完成正逆 apply/check 和精确字节、无关 sentinel 核对。

4105B/147行源全读，3内联测试、1 helper、2内嵌 snapshot 已读但0执行；10组映射，P01–P10未运行判据。17个当前目标片段，46输入快照；保留原报告、原 ready 收据、H132/H124通过和失败。

离线复验：python3 Docs/quality/stage1/ZS1-304/worker-pending-approvals-review-3.1.20/verify_package.py。只读此包并写自身临时 Git 仓库；核验全 manifest、结构、单报告 delta 和正逆字节，不访问网络或产品。

无 Cargo/PTY/HTTP 新运行，无主库/产品/蓝图/索引或验收勾选变更，不改303/301/302/306旧包；上下文不计额外文件完成。后台通知发现性、切回焦点条件、队列容量及 new 生命周期边界详见报告；结构检查不代替语义验收。

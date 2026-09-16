# ZS1-304 — pending_thread_approvals.rs 交互审查

源 `codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs`：4105B / 147L / `a401d7c6ba3051fcf2c20152a9e86a3f4f8751a87470cbc7f518001326618a2f`，完整读取 1–147 至 EOF。

该文件维护按 thread/session 归属的待审批请求集合，提供插入、移除、查询和 pending 状态投影。记录以 request/thread 标识区分，供 approval overlay 和上层 TUI 在当前线程切换时筛选。它只保存待处理元数据，不执行命令、不写权限、不访问网络。

关键边界：线程切换、请求完成、取消和重连必须由上层同步移除或失效旧记录；仅按 thread id 查询不足以防止 generation 重用。重复插入需要 request id 去重，否则同一审批可能显示多次。移除操作应保持幂等，未知 id 不应误删当前线程其他请求。队列为空时 overlay 应明确无 pending 状态，不能把旧焦点或旧 allow 结果复用。

建议 zenpi 采用 `(session_id, generation, request_id)` 复合键、append-only decision receipt 和默认 deny；取消/超时/断线时将 pending 标记为终结态，迟到确认必须被拒绝。应覆盖跨线程切换、重复插入、未知移除、取消后迟到事件、重连 generation 变化和空队列渲染负例。

本轮未运行产品、Cargo、测试、PTY、网络或 runner，未修改主控源码。

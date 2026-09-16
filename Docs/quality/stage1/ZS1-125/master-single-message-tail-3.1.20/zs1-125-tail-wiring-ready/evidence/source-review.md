# ZS1-125 wiring: independent full-file interpretation

## src/tui.rs (frozen master, 14,461 lines)

全文覆盖基线的 1–14461 行。读取期间没有改写冻结 before；工具显示截断的 11501–12000、12251–12500 段已拆小重读，不能把截断输出算作完整阅读。read-ledger.json 的分段 hash 是对已读原文的事后覆盖账，不是伪造逐段时间戳。

1–1500 行定义边界和投影：默认 2048 条消息，单消息256KiB，渲染8192行，提供者邮箱另有计数和1MiB预算。Session/Gantt视图各有自己的截断规则；文件补全、目录选择、复制浏览器和审批模态也分别拥有边界。TuiMessage 保存正文和可选 ViewBlock，TranscriptRow 才是显示层，position 不应替代规范正文。TranscriptUx 保存 scroll_anchor、anchored_position、message_offset 等交互状态。复制浏览器和审批预览使用旧的头部渲染入口，本次不能批量替换所有调用点。

1501–3000 行中，项目草稿不仅保存输入，还保存滚动、折叠、TranscriptUx、布局和流式状态。select_project_tab 在离开项目时 take 当前 ux，在返回项目时恢复对应草稿并使缓存失效。message_offset 因此归属项目，而不是跨项目的全局消息序号。checkpoint 是另一种持久化契约，不能从内存切换测试推断重启后锚点绝对一致。文件编辑令牌/DraftEpoch 保证迟到编辑不覆盖新输入；资源、会话等快照由各自的 owner 负责。

3001–4000 行是消息和流式变更入口。push_message / push_message_block 达到消息条数上限时从队首淘汰并递增 message_offset；clear_messages 才重置序号和锚点。append_stream_for_job 检查 job 身份，以既有可接收文本边界更新同一消息，刷新缓存；finish_stream 用最终正文替换暂存显示，过期 job 不可污染当前回合。follow_latest 清除两种锚点并将 scroll 置零。copy_latest_answer 从规范 Assistant 正文取值，显示裁剪不可回写正文。

4001–6000 行管理输入、滚动、鼠标和按键。scroll_up/down 显式导航时清除旧 source anchor，再由下一次绘制建立新位置；普通输入、括号粘贴、kill/yank、历史、外部编辑各有不同状态和边界。模态优先处理按键；没有输入草稿时 End 恢复最新跟随。页面上下滚动与 SessionList 导航的分支属于现有行为，不应为了 transcript 接线更改。

6001–7000 行建立渲染/布局关系。render_transcript_pane 在宽度变化或消息变更后重建缓存；先按 source position 在窗口里定位，找不到且已被窗口淘汰时夹到最早保留位置，否则沿用并夹紧数值锚点。它记录首行 position 作为下一帧的 anchored_position。transcript_messages 使用 message_offset + deque index 给真实消息赋 id，reasoning 折叠保留相同 id，工具折叠追加合成摘要。BentoBox 调用同一 transcript pane；Goal 也经 transcript_lines 接入窗口，但其滚动属于 pane_scroll。会话浏览当前是同步刷新，Gantt/资源有自身投影；这些是其他候选的职责。

7001–9500 行包含渲染调度、slash 分派、状态/资源格式化和项目运行宿主。调度器合并帧请求，不拥有 transcript 内容。ProjectRuntimeHost 切换项目的 Agent/session/cwd，运行意图、蓝图和队列命令交给持久化 owner。格式化的资源源路径、diff 正文等应在既有显示预算内尽量可读，不能为尾部渲染引入第二套运行 owner。

9501–12500 行是同步入口、输入控制票据、独立未来作业 FIFO、新会话控制和生产异步循环。队列保存原始输入和粘贴身份；提供者事件缓冲优先保留工具生命周期事件并显式报告丢失。生产循环按项目切换可见状态、排空事件、完成/取消 job、恢复草稿、保存检查点并绘制 BentoBox。外部编辑运行时让出终端事件所有权；退出必须恢复终端后关闭各 worker。本次 diff 不触碰其中的异步、会话浏览或生命周期分支。

12501–14461 行收尾同步循环、外部编辑、TerminalGuard/信号、事件投影、剪贴板和文本辅助函数，再定义 transcript_window 与内联测试。terminal_clipboard_sequence 仅编码显式复制请求；绘制没有剪贴板写入。truncate_bytes 保留UTF-8边界。view_block_text 将结构块转为显示文本；正文+附加块可能超过单正文大小上限，仍受 render 既有输入上限约束。

原 transcript_window 从最新消息向前遍历，以 min(8192, MAX_MARKDOWN_CELLS / width).max(1) 限制窗口，保留最后 remaining 行；若截掉旧消息/旧行，用首行位置替换为省略提示。问题是它拿到的 rendered 已被旧单消息渲染器从头截断，之后再取尾也无法找回真正的最新行。本次仅将两个分支统一接到冻结 tail metadata API，使用 rendered.lines，并以 omitted_visual_lines + local_index 记录完整视觉布局内的行号；omitted 用累计或运算合并单消息省略与多消息省略。没有按8192阈值混用两种换行算法，也没有重新从零编号滚动尾巴。总窗口、标记替换、persona 样式、项目 id 与 BentoBox 继续由原逻辑负责。

## tests/tui_transcript_ux.rs (independent frozen owner, 372 lines)

该文件全文372行，3个辅助函数和13个既有测试。key 直接调用公开按键处理；render 用 TestBackend 经过公开 render_bentobox；event 将 ProviderEvent 变为规范 ViewEvent。它不启动生产终端循环或网络请求。reasoning 测试区分推理与答案、折叠/复制和过期 job；usage 测试区分当前和过期 usage；block/clipboard/error tests 检查复制载荷、编码、大小和滚动检查器。

既有5项 transcript 窗口相关回归检查：新消息到达时保持阅读首行且End跟随、18000行多消息后最新答复可见、400条多行消息造成窗口移动仍保持锚点、消息淘汰加项目往返仍保持首行、旧历史裁剪标记和窄屏再放大。另有项目推理偏好checkpoint、窄项目tab鼠标箭头、目录picker优先拥有快捷键。这些已经覆盖主库接线周围的行为，但没有单消息自身超过8192视觉行的情况。

新增3个独立行为测试，保留所有13项原测试字节。第一项使用9000源行的User、Assistant普通Markdown、Assistant fenced code，检查最终尾部、规范正文未删、省略提示与End。第二项在8180行附近跨过8192阈值，以及已到9000行后继续滚动尾部，分别使用plain User和未关闭代码围栏Assistant；三批同job追加后保持同一阅读首行，再End看到新尾部。第三项使用一条包含CJK和组合字符的长物理行，在3/4/5/8/12列的公开经典render入口换行到超8192视觉行，拼接内容列验证LATEST后缀仍可见，再经BentoBox宽屏+End看到完整后缀。首次草稿误用了私有render_workspace_pane，被完整主库编译拒绝；修复到公开render后16项通过，失败原文和日志全部保留。

## Contract limits

准确视觉偏移针对 render 接受的最大256KiB显示输入。TuiMessage 正文本来按该字节上限截头；附加块拼接或更长上游字符串仍会进入 render 的既有头部字节上限。因此“最新”指被接受的输入范围，不能声称从任意大小原始字符串找回末尾。规范消息/journal不因显示裁剪删除。

锚点是固定宽度和稳定已解析前缀下的视觉行身份；宽度变化、修改旧内容或Markdown重新解释已写出的前缀可能改变视觉布局。既有缓存会按宽度重建，End可恢复当前布局最新位置；本次不新增源字节/语义锚点，不保证重排后的相同文字仍处于完全相同屏幕行。已被滚动窗口淘汰的锚点仍夹到最早保留行。全屏宽度极小、pane无内容高度，或行预算仅剩1行且省略提示占据该行时，不能同时显示完整提示和尾部正文；保留主库既有预算/标记语义。

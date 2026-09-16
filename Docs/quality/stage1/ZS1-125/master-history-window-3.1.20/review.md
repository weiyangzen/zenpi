# ZS1-125：长会话保留最新消息和阅读位置

修复真实TUI显示错误：历史超过约16,384视觉行后，provider新回复已写入会话，但Conversation仍停在较旧消息中间，End也无法显示最新内容。旧transcript_lines从最旧消息开始，到两倍MAX_RENDER_LINES就返回，随后主面板又从这份旧前缀取尾，真正的新消息从未被渲染。

## 实现与范围

src/tui.rs从最近消息向前构建显示窗口，再恢复消息和视觉行的正序；达到窗口上限后不继续渲染更早消息。缓存最多8,192行，并按当前宽度收紧到4Mi近似terminal cells。被窗口省略的早期行在窗口顶部有可见提示，提示本身按宽度截断。这里的cell预算不是整个程序RSS/字节上限，也未替代单条消息renderer内部限制。

滚动锚点改用消息序号和该消息内视觉行索引。队列pop_front时累计消息偏移，项目草稿保存各自TranscriptUx，因此追加、窗口前移、消息队列淘汰及项目切换不会把仍保留的阅读位置推走；原位置已被窗口淘汰时限制到剩余窗口顶部。显式滚动重新选择锚点，End清除锚点并继续跟随。宽度改变时仍按消息内视觉行定位，不承诺同一字节在任意重排后保持像素位置。

Canonical消息队列、会话日志和复制来源没有因显示窗口而删除或重写。Reasoning/工具折叠仍是只读显示投影。没有更换BentoBox、项目strip、目录picker或审批逻辑。修改仅src/tui.rs、tests/tui_transcript_ux.rs和已有tools/tui_transcript_ux_smoke.py（新增--long-history-only）。

## 失败证据与验证

旧c30c33edb95ae962148d84acc2564a265a8f3b0120f95fa3590cee43b13eabf5 native ARM debug上：一个真实headless进程经3次loopback Responses调用创建三个各6000行的历史回复，随后一个真实TUI发出第4次请求。新回复TAIL_LIVE_FINAL已存在journal，End之后屏幕却停在P3_4367附近，latest_reply_visible_beyond_18000_rows=false，探针exit1。其它4项检查通过，终端正常恢复，HTTP数精确4，没有fixture超时。before日志、原始PTY、JSONL、HTTP和journal均保留。

旧源码上的两项新增Rust回归实际失败：最新回复不可见，以及滚动窗口前移一行导致history-398-04变成history-398-05；另外9项旧测试通过。修复后追加队列淘汰/项目恢复和窄屏/窗口提示测试，共4项新Rust测试。

最终同一native ARM debug 98987fe27d6f15a6fd45ce3f35d1adbeb694edc78effb6e418ff4de3389a06cc验证：

- 83项Rust通过：tui_bentobox16、tui_command_palette17、tui_resize14、tui_transcript_ux13、TUI库23。重复中间轮次不累计。
- 同一新长历史PTY5项全部通过：阅读位置保持、End显示最新回复、原历史与新回复均留在journal、4次provider调用、终端恢复。1headless+1TUI/4HTTP。
- 原有transcript实际入口11项通过/3HTTP，包括live scroll、鼠标Latest、reasoning、usage、answer/code复制、工具和provider错误查看、inspector取消、窄屏项目切换与终端恢复。没有减少原断言。
- 严格库Clippy、rustfmt、Python语法检查通过。原vendor crossterm警告保留。

202份代码/测试/工具/vendor验证输入在构建后、真实入口验证前冻结，发布前逐项检查；不写成构建前供应链快照。三文件精确增量patch独立Git正逆apply/check、字节比较和无关sentinel保全通过。全部失败输出保留。

## 仍未闭合

此包修复多条消息组成的长会话窗口。单条回复本身超过8,192视觉行时，既有src/render.rs仍可能只返回该条消息的前缀；该独立文件的尾部渲染和精确省略行元数据已交给原任务B实现，待主控接线、复验后才能声称单条超长回复也已闭合。该包没有把独立消息的内部截断伪装成全部解决，也不接受096或085逐文件报告。

本轮没有重建release或重复冷启动预算。旧release8/8不覆盖此前headless、idle菜单及本次窗口改动。上一轮全库90通过/1失败/5忽略的扩展超时夹具失败仍保留；原任务A在独立目录诊断，尚未合入，不以本轮TUI专项通过覆盖。原任务C正在处理CI预算失败后的证据保全，也未被此包接受。

ZS1-125仍为[ ]，阶段仍21[x]/0[_]/91[ ]。本增量通过不代替逐文件/目录闭包、全部交互矩阵或最终生产验收。回退须先检查后续修改，再反向应用product.patch；不reset仓库、不删除用户会话/项目或任何原ready。

# ZS1-123：后台刷新保留用户选中项与会话搜索

本增量修复两个实际TUI问题：用户用方向键选中第二项、停顿后确认，命令菜单会退回第一项，会话浏览器也会打开第一条记录。保留BentoBox和项目加号选择工作目录的交互。

## 实现

- 输入队列的相同Page、相同Input和Modes回复不再重置菜单或重复请求重绘。已有Input按ID原位更新；变化影响当前菜单时按完整选项文本保留选择，并在重新绘制前禁用多选项Tab确认。其它菜单不因队列变化丢失选择。
- 会话列表按路径保留选择，记录删除时将索引限制到剩余记录。搜索查询、结果、选择与扫描时刻存入各项目的运行时草稿；项目切换后恢复，空结果不会被idle刷新覆盖。显式新搜索从首项开始，`/session list`解除过滤；失败的新搜索保留上一份结果。
- 同一owner的目录扫描最多每500ms一次，最多保留32条显示记录；owner/目录变化及明确搜索/列表命令立即刷新。修改了旧“no I/O”注释。一次扫描仍使用现有同步list_sessions/search_sessions，并非异步索引，不能声称大型目录零阻塞。
- 模型目录到达或内容变化时显式请求重绘；相同目录不重复重绘。选项变化保留可匹配的选项，并要求重新绘制。这使模型菜单不再依赖队列轮询持续置dirty。

仅变更src/tui.rs、tests/tui_command_palette.rs与已有tools/tui_command_palette_smoke.py（新增--idle-selection-only）；没有更换布局引擎、取消审批或改写项目cwd。

## 真实失败及验证

旧native ARM debug二进制bb82450f8de71a0a7a91d828e42f659566cae3c7e4e574dd68706bfa10b94172上的真实PTY：方向Down后停顿1.2秒，Tab得到/persona INTJ而非INTP；搜索结果第二项同样退回第一项。3次loopback HTTP仅用于通过真实headless owner预建3个含搜索词的会话，导航自身没有HTTP，终端正常恢复。旧源码上的两项新增Rust回归也实际失败（另1个匹配过滤器的旧测试通过）。原始失败和全部输出保留。

首次局部修复8c383966…的定向PTY通过，但完整菜单流程未显示/reasoning high而失败；该失败、HTTP和PTY保留。随后补模型目录变化的dirty通知，新增独立“目录到达触发绘制、重复目录不持续绘制”单测，并让原smoke明确等待键入草稿已持久化、记录该时刻checkpoint。没有删除或放宽/reasoning high断言。这次完整流程通过，并不把单次UI超时的所有时序因素都声称已定位。

最终产品源码通过73项Rust测试（TUI库23，project_workspace7，tui_approval_focus12，tui_command_palette17，tui_resize14）。其中本次新增5个测试，覆盖idle已绘制选择、动态队列变更/重复ID、过滤与空结果、跨项目/外部新增/删除和模型目录绘制。计数不重复累计中间轮次。

实际入口分开绑定二进制：

- e0def1d182efe09ef45d2161c26544f6e27516065c1566decf1a679375f70daf：完整菜单24项检查/12HTTP，BentoBox11条记录（9布尔、resize列表及plus对象）/3TUI/0HTTP，项目picker12项断言，审批14项/12HTTP，停顿选择4项/3HTTP。所有终态实际成功。
- 此后严格Clippy指出搜索分支可合并if。只做等价let-chain整理并格式化，最终c30c33edb95ae962148d84acc2564a265a8f3b0120f95fa3590cee43b13eabf5上重跑73项Rust和停顿选择4项/3HTTP，通过严格库Clippy及rustfmt。tui-before-clippy.rs与最终src快照提供该机械差异；没有把上一二进制的整套入口输出改标为最终二进制。

三个文件的精确增量patch在独立临时Git根正向/反向apply-check后逐字节验证，并保留无关sentinel。202份代码/测试/工具/vendor输入在各次构建后、对应验证前冻结，发布前复核；不是构建前供应链证明。

## 未完成边界

本轮全库测试实际90通过、1失败、5忽略：extension_runtime::pipe_deadline_tests::escaped_pipe_timeout_is_bounded的stdout夹具未进入逃逸状态（fixture never escaped）。原始日志保留，不能以TUI专项通过覆盖全库失败。首次严格Clippy的collapsible_if失败也保留，已修正后通过。

本轮未重建release或运行冷启动预算；上一release的8/8预算只绑定旧1d7b2e61…，不能覆盖本次TUI及此前headless修复。下次release需要包括两者。会话浏览搜索/选中位置仅在同一进程的项目间保留，未新增跨重启持久化承诺；长历史显示截断、完整目录依赖与其它蓝图项继续独立处理。

ZS1-123仍为[ ]，完整目标未结束。本证据只接受此实现增量，不替代085逐文件主控复核、目录闭包或阶段验收。回退前核对后续修改，再反向应用product.patch；不reset、不删除用户项目、会话或原ready。085候选冻结于旧d2a源码，接受时应引用本新证据，不能改写原报告。

# ZS1-126 会话列表选择修复 · 主控局部整合

滚动到会话列表中部后，原界面显示的是第8条，点击却选中了第0条；窄栏中换行又让“当前项”的逻辑行与屏幕行不一致。主控在真实 SessionStore 文件、实际 TuiState 鼠标/键盘入口和 TestBackend 渲染上复现两个反例，未修改产品时0通过/2失败（exit101），完整原测试源码、输入和日志保留。

当前显示与点击共用 session_browser_scroll，鼠标加上相同的可视起始偏移，且只接受内部内容区点击。SessionList保持每个会话一行，过长标签按栏宽裁剪；其它面板保留原来的换行。列表仍最多32项，选择仍交给原OpenSession宿主路径。没有修改BentoBox布局、比例、断点、折叠、顶栏项目身份或加号选择工作目录行为。

三个新测试核对：屏幕上可见session身份→点击→实际选择path→Enter返回同一OpenSession；120×30、120×20、300×40缩放后选中标记仍在正确行；边框和空白行不改变选择。草稿和当前布局保持。两个原失败的语义断言未放宽；第三个边界测试在修复后新增，无虚构before结果。源码仅src/tui.rs与tests/tui_project_workspace.rs，精确差异/root.patch和初始202个构建输入身份已归档。

主库原生stable-aarch64-apple-darwin，locked/offline执行9个相关测试目标，共143个不同Rust测试全部通过（包含3个新测试）；fmt、strict Clippy lib+project-workspace目标、debug build均exit0。所有构建前后202个输入一致，现存vendor unnecessary-parentheses警告未修改。before测试二进制、after测试二进制与新debug二进制各自保留身份；写入修改使用新mtime，未复用旧产品冒充新构建。

这是局部UI选择修复，不是本轮完整真实PTY/生产release/冷启动预算或跨项目恢复认证。本轮没有新增PTY/HTTP；跨项目shell用例属于实际143项Rust回归的受控子进程场景。原异步会话浏览修复和历史测试留在master-async-browser-3.1.20；旧117首启失败仍保留。完整126、128及全Stage1仍未验收，清单50/121不因这处局部修复增加。

新debug:50751592 B，SHA256 58bb7e5ffd95fe5db337ba2b86cecc0ec40ddf78843576b3c48938c3acd00b61。当前TUI6459360343a260c6dfe7b403a81cd22a8fcac0bb3e10fcabed2bd3c0b8567254从e451增量整合。随后122 history Super修复必须仅应用它自己的guard，不能覆盖本修复。

回退只反向应用这两个路径的root.patch且先核当前差异；不能恢复整文件覆盖后来修复，不删除会话或历史证据。主控已独立阅读全文相关函数及现有完整project-workspace测试，未将此有界产品工作冒充整个TUI/目录逐文件验收。

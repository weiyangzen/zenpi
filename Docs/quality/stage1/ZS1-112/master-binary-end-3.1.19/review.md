# ZS1-112 二进制结果修复 — 主控定向记录

当前 src/search.rs SHA256 20217e87ca2427fcdf6a5ed6fffaf6c481ed5c12d5d485752bbcae6f89d358cf 保留binary_end处理；不包含被拒绝的新增depth probe。主控完整读取668行原候选search.rs及每处差异、121行新增测试、完整公开CLI语义/取消脚本，验证648路径冻结payload/base/单文件patch，完整2074458-byte patch在私有Git中实际正反向应用并逐hash还原。

实际旧主控生产release7a374...在14字节prefix\0needle\n输入上返回text needle、truncated=false；70层目录同样无结果但truncated=false。两个新回归在未改产品时各失败0pass/1fail，两个独立CLI+HTTP入口也各复现行为失败，非编译/夹具启动失败。完整候选一度通过规定22+35+8=65顶层测试，但后续审计证明新增depth walker会阻塞于并发出现的.ignore FIFO，取消2秒/整体期限10.5秒都不能返回；C另存实际生产反例。主控因此撤回仅新增depth_target、调用和方法，保留独立binary_end修复。历史65pass不能支持当前完整112验收。原深度缺口与新增深度回归仍未闭合，C正在实现安全的规则读取，未删除失败断言或降低要求。

二进制修复消费rg end.binary_offset，先验证路径归属/已准入batch/private inode，再删除该文件之前累积的match/context并报告binary_content。保留已有二进制行检测。原JSON流/文本界限不变。撤回depth probe后，独立二进制回归再次通过；主控scoped严格Clippy与cargo fmt通过。

主控当前debug CLI 24721d51a33955057380e20126cbc569706ac17b82c3ce8c370a059a19674c77 实际构建并固定后，7个语义场景通过：binary、regex/context、ignore/hidden、literal/glob、find/glob、match_limit、.rgignore过滤。binary从原失败转为无matches/context并显式truncated；其它场景保证现有语义保持。每次实际HTTP中的tool结果与持久journal一致，独立验证所有artifact-index字节数/哈希与进程身份。该debug尺寸不用于release预算。

实际公开取消/重启另一次通过，取消到terminal 118.07ms，真实leader和普通后代kill(pid,0)均ESRCH，仅1次HTTP，第二CLI按原ID回放同一terminal bytes，无自动重复执行或partial index。受控rg wrapper在真实工具owner中启动sleep以固定取消时机，不把sleep当作搜索语义。HOME/CODEX_HOME保持，私有ZENPI_HOME和固定binary哈希已核对。

所有131个当前源/vendor/Cargo/指定测试输入重验。外部编辑器130与phase test-only补充已在当前debug内；整项112、深度探测、全Rust/预算与源/目录依赖均未宣称完成。回滚仅对search.rs当前binary_end定向差异，保留原用户修改与已登记待修复测试。Gantt可显示此局部修复，但仍是[ ]。

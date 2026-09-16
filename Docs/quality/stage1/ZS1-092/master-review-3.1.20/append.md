
## 3.1.20 主控当前目录重核与接受（2026-09-12）

以上11196字节为worker历史候选，精确前缀保留。其“089未接受”“当前CI94行”“只上传/tmp单JSON”“预算失败跳过上传”等说明限定于捕获时点；本节给出当前状态及替代结论，不抹除旧证据。主控已完整阅读上述73行报告和同目录release.yml全部87行，并复用亲自完成的089完整文件/227行报告审查；不是从候选标题或哈希推断理解。

当前权威仍为3.1.20、digest `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`；本目录接受前快照为 `f6fcc84d9360cb6a1cd444bb93b544371e07f0d0192dedd67029be153b2d53ae`。ZS1-089已由主控独立接受为[x]，当前标准报告37832B、SHA256 `cad5882b88c6014a5d9d9f755e63cdcbf4a89b424801cbc0d52cc86fabcf0891`，master receipt实际存在并重新按当前authority校验。其主控审查位于 `Docs/quality/stage1/ZS1-089/master-review-3.1.20/review.md`，实际G-STAGE --item ZS1-089退出0；不能把此状态反写到历史dependency089包。

本轮再次即时枚举直属entries：仍仅两个regular文件，没有symlink/特殊文件/直接子目录。in-scope ci.yml现在3973B/113行、SHA256 `44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9`；context-only release.yml仍3779B/87行、SHA256 `caaf9eb891dc6636229ed9a07e3474d6f3a8823f6670d2a3b4d2954cce0d9cbd`。冻结直接文件集合{089}、直接子目录集合空、蓝图Depends{089}三者一致。release.yml确实存在但未被计作新增L1或已接受源文件，本次仅接受冻结目录子集。

CI相对历史94行候选只改变预算和上传两步骤。预算显式Bash，使用runner.temp/run_id/run_attempt独立目录，合并实际stdout/stderr到budget.log，立即保存PIPESTATUS，在可达的退出分支优先返回预算失败码、否则tee码。exit-code.txt只记录预算码；summary与startup诊断可能因早期失败缺失。上传使用always且预算outcome非skipped，上传同一证据目录、缺文件报错；不再是单/tmp JSON或仅正常成功时上传。目录创建失败但旧目录存在时仍可能上传旧目录；状态写入失败、runner丢失、强制取消或超时不保证上传可完成。这些当前边界继承已接受089的独立分析，不在092重复宣称五次新的shell测试。

目录级调用关系保持：ci.yml的单verify job与release.yml的matrix package是两个独立入口，没有workflow_call/workflow_run或显式跨workflow成功依赖。两者共享仓库路径和release.sh调用，但各自重新构建，CI生产包不作为release工作流直接晋升输入。CI预算证据目录属于本次runner临时状态和Actions artifact，release的dist归档/校验和上传及tag附件是另一产物链；不能用相同源码、工作流目录或文件名代替被测试/被发布二进制的hash绑定。

CI permissions仍contents:read、workflow/ref并发取消、15分钟；release仍contents:write、manual/tag触发、五条matrix声明、fail-fast=false，Unix调用release.sh，Windows用PowerShell独立构建/打包/SBOM/校验，随后平台smoke、upload和tag release action。源码中的matrix条目不证明运行器现时可用或实际运行通过。release没有共享CI的concurrency锁，也没有本目录的跨平台全局提交/撤回事务。权限、取消、缓存与product session/reap语义不可混同。

CI归档检查已使用逐归档tar成功和grep错误分支；release的Unix smoke另有SBOM前置检查和否定pipeline，Windows使用独立listing/checksum表达式。主控只读完整release上下文，没有执行它、套用089坏归档反例或声称其验证完整。production/fixture身份、checksum与上传对象对应、运行时来源hash链，以及真实远程平台结果仍需相关产品/发布门禁证明。

本次实际检查是：旧092包95个payload的字节/摘要，历史frozen verifier退出0，当前即时目录清单与两文件全文hash，089实际master/worker receipt和当前report/source绑定，正式直属范围/依赖闭包，以及本目录新增最终报告的临时Git正逆check/apply与sentinel。没有运行Cargo、PTY、HTTP、workflow、release、预算或坏归档行为案例。结构checker不替代主控语义审查。

主控接受的结论限定于ZS1-092目录关系已理解且冻结唯一子文件089已[x]；本节与独立master review/receipt构成当前接受依据。ZS1-093尚需另一次父目录清单、作用域和当前092证据审查，不因本项闭合自动通过；release.yml和ZS1-117也不随之接受。当前Stage1 checker接入CI、归档负例持续回归、hosted上传/新release预算等余项保持开放。回退本目录报告与本次证据/状态不得撤回089、改产品或覆盖历史候选包。

# ZS1-092 · 目标 .github/workflows 目录集成候选

**Provisional，依赖未闭合，不能验收。** ZS1-089已经由worker完整复核并冻结候选，但主蓝图仍为`[ ]`，主库没有其报告或master receipt。本报告仅在主控明确授权下提前准备目录关系，不满足G-DIR的子项`[x]`前提；不把候选收到、完整性通过或正逆patch通过当作主控接受。

## 正式范围与直属清单

权威3.1.20，requirement digest `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`。蓝图ZS1-092目标为`target:.github/workflows`，L2，仅Depends ZS1-089；唯一owned report为`Docs/learn/stage1_pi_mono/targets/zenpi/.github/workflows/current_folder_learn.md`。回退仅本目录报告/新增证据，不递归撤销子文件候选。本轮没有跨到`.github`根整合，也不接受ZS1-093。

G-DIR原文要求“直属文件与直接子目录均已 `[x]` 后”再独立整合冻结子集，并记录in-scope/context-only、调用、所有权、错误/取消/持久化和跨文件不变量。冻结manifest只把ci.yml分配给089；实际目录另有release.yml。按蓝图范围规则，后者可以解释上下文，不能因恰为直属文件而自动增加L1接受项。若主控将发布工作流行为升级为交付义务，需另行扩版和独立owner；本候选不自行扩大范围。

捕获时间2026-09-12T11:37:59.185516+00:00。仅即时列出主库该目录直属entries并逐项lstat/hash，没有递归扫`.github`或其它owner。主库与原worktree均为两个regular files，无symlink、特殊文件或直接子目录。

| 直属项 | 分类 | 捕获身份 | 接受状态 |
| --- | --- | --- | --- |
|ci.yml|in-scope，ZS1-089|3104B，94行，`b9e70ba448d157341e0069fcd9ad19eae8f0412aaf0b9e03bbd97b4f2ef59dbd`|worker候选`[_]`；master未接受|
|release.yml|context-only，无本冻结子集L1项|3779B，87行，`caaf9eb891dc6636229ed9a07e3474d6f3a8823f6670d2a3b4d2954cce0d9cbd`|不作文件或目录接受声明|
|直接子目录|实际与冻结均为空|即时目录清单|无可漏掉的子目录依赖|

因此冻结直接依赖集合严格等于`{ZS1-089}`，不是把release.yml藏掉，也不是声称整个现实目录都已验收。形式上的目录树闭包由蓝图checker的直属集合核对支持；语义接受闭包目前不成立。父目录在正式图中为ZS1-093，仅记此接口，未分析或写其报告。

## 先核对子文件候选，再形成目录结论

本轮先完整重读089报告：全部3.1.16旧前缀及全部3.1.20当前节，再完整核对当前ci.yml94行，最后读取同目录release.yml87行作为上下文。没有从089标题或manifest一句scope推断内容已掌握。

089候选manifest `d82de0618e7c6aa00062a0df039ad4016473dcd5c9eba5cf798962245235674c`，报告24526B SHA256 `33e353712730db9dcb9335e8c16c8d9caa83c2488a0d7b851d3042a33dfa670f`；74个member及manifest完整复制到本项dependency089，校验所有字节并保留候选原样。对应current-ci.yml与本轮目录ci.yml逐字节相等。其正式基线仍为77行2480B `5d7173fa20a0de949e53525018dad07718a9e4a9d24a5fcd137b056af31792d6`，不能拿94行改写冻结基线。

089报告当前节已明确旧归档pipeline缺陷修复、user_smoke当前含production与fixture两段、历史回放平台和仍未覆盖场景。旧前缀中“current=77行”“待修归档pipeline”只属于旧时点，不进入本目录当前结论。089的实际live/frozen证据校验退出0，G-STAGE退出1仅记录缺089master，不能推导独立文件通过。主库报告路径和`Docs/learn/stage1_pi_mono/receipts/ZS1-089.master.json`在本轮捕获均absent；主控收到候选的会话消息不等于写入/签署验收。

## 目录调用关系与执行入口

in-scope ci.yml是push/pull_request触发的单verify job；顺序调用格式、Clippy、Rust tests、旧v1/v2蓝图、LOC inventory、runtime budget、modes、debug JSONL、installed user smoke、release JSONL和release.sh包装。workflow不实现这些工具的语义，只决定调度、参数、环境和成败传播。089的hash绑定调用片段继续作为当时上下文；本目录不再次横跨读取tools实现，也不把其全文接受迁移进来。

context-only release.yml是workflow_dispatch或v* tag触发，具有package job的五行matrix声明，Unix分支调用同一个tools/release.sh，Windows分支在PowerShell中build、拷贝binary/README/LICENSE、生成SBOM、zip和checksum；后续做平台对应smoke、artifact上传，并在tag条件下调用发布action。这里的matrix只是源码声明，不证明这些runner当前可用或五个平台实际成功；本轮没有触发workflow/发布。

两个workflow没有互相workflow_call、workflow_run或显式跨workflow成功依赖。ci.yml不调用release.yml；同目录不是调用链，发布workflow也没有在本文件中接上ci.yml的验收输出。不能从某次CI归档成功断言发布matrix每个产物已接受，也不能从发布matrix推导CI具有多OS验证。实际保护规则、环境审批或外部branch protection未读取，本报告不否定它们可能存在。

目录内共享的是仓库revision、Cargo.lock以及tools/release.sh等被调用路径，实际artifact有不同构建和存储生命周期。CI先后含fixture-enabled构建和production packaging，089已说明默认production package独立build；release.yml的package也独立构建，并非下载CI产物再晋升。源码相同只提供输入关系，不能建立“测试二进制即发布二进制”的hash链。

## 所有权、数据与跨文件不变量

ci.yml声明contents:read；release.yml声明contents:write并含tag发布步骤。权限属于相应workflow执行上下文，不可把CI只读概括为整个目录无外部写入。候选worker只读主库与所有源文件，没有调用任何upload/release工具。本报告自身是本地文档/证据，不是这些workflow动作的授权。

Rust build/test/cache属于runner环境；provider/session/approval的持久语义归产品owner，不能被workflow concurrency替代。CI并发group带workflow/ref且cancel-in-progress=true；release.yml没有相同concurrency块，matrix fail-fast=false是不同的调度策略。两者不是共享执行锁，也没有目录级atomic transaction。

CI budget写/tmp汇总后只配置此JSON上传，sample诊断目录由调用脚本产生但未显式列入上传。CI发布产物检查在dist完成，此workflow未另外上传production归档；release.yml上传dist的tar.gz/zip/sha256，并在tag触发发布。两个不同artifact通道不得相互替代，预算receipt也不是发布来源证明。

CI最终归档检查先确认tar成功、再检查禁止名称并识别grep错误，临时listing由EXIT trap清理。release.yml的Unix smoke片段同时有前置SBOM tar/grep检查和另一个否定pipeline，Windows分支另有checksum/listing逻辑。不能只见相似旧pattern就把089的“损坏归档会通过”反例移植到release.yml：其前置步骤、显式shell和数据流不同，未独立复现。这里只记录两入口没有共享同一个归档验证实现，存在后续维护一致性需要；不提交release.yml修复，也不接受其安全行为。

重要跨文件不变量是：检查的archive必须与checksum和最终上传对象一一对应；同名workflow或dist目录不代表同次运行产物；production/fixture身份不能混同；只读CI权限不覆盖发布写权限；成功构建/上传不等于蓝图master receipt；临时文件清理不等于zenpi运行时子进程reap。当前候选只解释这些边界，未用真实Actions或产品执行证明它们全部满足。

## 错误、取消、持久化与恢复

CI中的build&&smoke保证本行build失败不执行smoke；普通后续步骤依赖成功，budget非零可能跳过其后的默认upload。这个行为说明来自089已保留的官方文档访问记录与源码分析，本目录不新增联网验证。CI15分钟timeout和cancel-in-progress可中止流程；源码只为archive临时清单提供EXIT清理，不给出全部日志的失败保留规则。

release.yml每个matrix运行有自己的staging和上传，fail-fast=false不要求某项失败立即停其它项；目录内没有定义所有平台产物的全局提交/回滚事务。tag发布步骤的外部作用不能由本地patch reverse撤回，本轮也没有触发它。失败重跑是重新执行命令/构建，不是恢复zenpi session或对既有用户会话的重放协议。

本目录的持久状态分为Git中的workflow配置、runner临时构建/diagnostic文件、Actions artifact与可能的发布附件；四者不同。worker报告与证据在原ff51工作树新增，不改变任何一类实际运行状态。HOME/CODEX_HOME保持；历史user_smoke child环境规则不推广为本轮环境变更。

## 未闭合项与后续顺序

首要未闭合项是**ZS1-089主控验收**。目录report已可供审阅，但G-DIR acceptance_eligible=false；即使主控稍后接受089，也需重新核对目录快照/当前CI/报告hash，再独立接受092，不能自动升级。092当前主蓝图也是`[ ]`，并缺自己的master receipt。

089保留的具体静态差距为预算失败后的receipt及startup诊断上传、当前stage1 checker接线、归档负例持续CI回归、托管Ubuntu证据和版本身份范围。它们没有因目录报告出现而修复；本目录新增的是两workflow独立入口与产物/权限/验证逻辑的关系说明，不另造重复L1完成数。

release.yml为context-only，未列入冻结文件完成率。若其发布验证成为新义务，由主控先扩版，再给独立文件owner与真实回归；本次不跨`.github`根做总体安全审计，不把其它队列中L1候选当作已接受依赖。本单没有需整合的直接子目录，不能拿空子目录集合回避唯一ci.yml未接受的事实。

## 验证、接收与回退

本轮真实行为运行0：没有Cargo、PTY、HTTP、Actions、release、坏归档回放或新测试。只运行独立静态完整性checker及只读G-STAGE，退出码分别记录。静态checker的ok只表示“provisional资料与未闭合状态一致”，绝不输出G-DIR验收通过。G-STAGE因缺092master返回实际非零；其首先报receipt缺失并不表示已检查通过089依赖，独立dependency-status明确记录该依赖未接受。

证据包保留即时两文件清单/全文、蓝图相关范围行、正式直属依赖集合、089完整74个候选member及manifest、接收状态、原worktree两个workflow hash、67份既有ready摘要、独立live/frozen验证器、真实日志和此报告hash。冻结依赖产物是审阅快照，不是把089再接收一次；所有源候选字节不变。

主库与本地原先无本目录报告，精确接收基线absent。新增报告与证据补丁仅在临时目录正向apply/check、逐文件hash、逆向check/还原；不force主库路径，不改产品/蓝图/其它owner。回退只删除本次新目录报告/证据，不动089、096或任何旧ready。完成后交主控并停止，等待子项闭合后的独立验收安排。

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

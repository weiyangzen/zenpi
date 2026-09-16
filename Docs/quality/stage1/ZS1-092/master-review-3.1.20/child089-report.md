# ZS1-089 — .github/workflows/ci.yml

Worker candidate: [_]. learn_mode: understand. Main semantic review required. Authority3.1.16 / requirement digest `c0262492bc6e3b4d5f56b35bedc7c1d1658854df11c4a975eb612a514cceef8f`.

source_path `.github/workflows/ci.yml`; original blueprint and current read-only main are both2480bytes/77lines, SHA256 `5d7173fa20a0de949e53525018dad07718a9e4a9d24a5fcd137b056af31792d6`. All77lines were read sequentially, including every blank/comment, trigger, permission/concurrency field and each of16steps. The current bytes equal the frozen blueprint hash, so the original content is preserved identically rather than replaced by a newer CI description. No prior local/main report existed. Exact byte interval/capture time are in worker-owner-review/input-manifest.json. Main reports116 integrated/production checks passed;122 remains scratch-only. Workflow bytes do not change merely because other owners' code changes.

## Triggers, execution boundary and every step

Lines1–5 name the workflow zenpi CI and declare push and pull_request events without branch/path filters. There is no workflow_dispatch, schedule or other event in this file. Lines7–8 request contents:read. Lines10–12 group concurrency by the literal zenpi prefix, workflow name and github.ref, and enable cancel-in-progress. This is workflow-run concurrency, not a resource lock inside zenpi or cancellation semantics for its sessions. Lines14–19 contain one job, verify, displayed as Format, lint, test, and contract checks, on ubuntu-latest with a15-minute job timeout. There is no OS/version matrix, job dependency graph, container, custom shell or working-directory override here.

| Lines | Step | Actual command/action and boundary |
| --- | --- | --- |
|20–21|Check out source|actions/checkout@v4, with no input overrides. This is runner checkout, not the worker's shared-main checkout.|
|23–26|Install stable Rust|dtolnay/rust-toolchain@stable requests rustfmt and clippy. Stable is selected here; there is no explicit MSRV/nightly/version matrix.|
|28–29|Cache Cargo artifacts|Swatinem/rust-cache@v2 with no explicit cache-key/path overrides. A cache action is not a correctness receipt.|
|31–32|Formatting|cargo fmt --all -- --check; check-only rather than rewriting source.|
|34–35|Clippy|cargo clippy --all-targets --all-features -- -D warnings. Warnings are errors. This command has no --locked flag, unlike the test command.|
|37–41|Tests|cargo test --all-targets --all-features --locked -- --test-threads=1. The comment explains serial libtest scheduling for short-lived loopback fixture contention. It does not create an OS/toolchain matrix or prevent tests from internally creating threads/processes; ignored tests remain ignored unless explicitly invoked by a parent.|
|43–44|Execution blueprint/Gantt|python3 tools/validate_blueprint.py, without arguments. Inspected script defaults target Docs/Zenpi_Execution_Blueprint.md, Execution_Spec and Execution_Gantt; this is the older validator, not the stage1 checker.|
|46–47|v2 review draft|python3 tools/validate_blueprint_v2.py defaults to Docs/Zenpi_Execution_Blueprint_v2.md and calls it a non-authoritative review draft. It does not validate the current stage1 blueprint merely because this workflow runs on current source.|
|49–50|Rust inventory|python3 tools/check_rust_loc.py reports physical Rust source inventory. Its inspected default is informational; no --max-lines argument is passed. Do not call it an aggregate LOC rejection gate.|
|52–53|Runtime/size gate|python3 tools/bench_runtime.py --samples3 --output/tmp/zenpi-runtime-budget.json. Script context includes8limits, builds/measures and atomically writes its receipt, then returns failure when the report fails unless no-fail is selected (not selected here). Three samples are not p95 evidence.|
|55–59|Receipt upload|actions/upload-artifact@v4 uploads that fixed/tmp JSON path as zenpi-runtime-budget-${github.run_id}. Only this receipt is configured for artifact upload; neither release archive nor alltestlogs are uploaded by another step here. There is no if:always condition to retain the receipt after a failed preceding budget step.|
|61–62|Two-mode boundary|python3 tools/check_modes.py. Inspected context checks RunModeTui/Headless and builds/inspects the binary; it is not a prohibition on auxiliary core CLI commands.|
|64–65|Debug JSONL smoke|cargo build --features dev-fixtures then, only if build succeeds via &&, ZENPI_BIN=target/debug/zenpi tools/headless_smoke.sh. Build has no --locked on this line. No allow-skip flag is passed.|
|67–68|Installed release user paths|ZENPI_SMOKE_FEATURES=dev-fixtures python3 tools/user_smoke.py. This explicitly requests fixture-enabled user-path smoke. The script's isolated_env changes child HOME and removes CODEX_HOME before setting ZENPI_HOME; those legacy fixture choices cannot be represented as complying with this task's preserve-real-HOME/CODEX_HOME rule. This review does not execute it.|
|70–71|Release JSONL smoke|cargo build --release --features dev-fixtures then ZENPI_BIN=target/release/zenpi tools/headless_smoke.sh --release. This build is fixture-enabled, and success does not alone prove the later production artifact excludes fixture functionality.|
|73–77|Production package/checks|Run tools/release.sh; enter dist and verify every matching *.sha256; then negate tar-list piped to case-insensitive regex grep for auth.json/.codex/.zenpi/fixture/.env names. Actual shell/negative behavior is analyzed and exercised below.|

All referenced local entry scripts exist in main at capture; path/byte/hash inventory and limited inspected excerpts are evidence, not a full sibling-script review. Action refs use major/stable labels, and ubuntu-latest/toolchainstable are not immutable version identities; no exact resolved image/toolchain/action commit or successful hosted workflow run is claimed. No deploy, release publish, push or notification step is present.

## Failure, cancellation, shell and artifact boundaries

GitHub documents unspecified Linux shell as `bash -e {0}`; explicit shell:bash additionally enables pipefail. This workflow supplies no shell override. Normal later steps depend on earlier success, so its receipt upload is not guaranteed after a budget failure. These shell/default-step statements were checked against [GitHub's workflow syntax reference](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax); the evidence records the source URL/access date. This is deliberately not an assumption that every run block starts with pipefail.

The build&&smoke pairs stop before smoke when build fails. The multiline package step relies on shell error handling for release.sh and checksum failure, but the final `!` negates a pipeline status. That negated condition has its own semantics and is not guaranteed safe by general fail-fast behavior. Workflow timeout/cancel-in-progress can stop a run, but the file has no always-run cleanup, diagnostic upload or durable recovery procedure. It does not establish that host subprocess/session cancellation/reap invariants hold; those must be tested in zenpi owners. Re-running a workflow starts its commands again; a restored cache is not journal/session recovery.

The fully inspected release.sh context stages only the built binary, README, LICENSE and SBOM, creates a tar.gz and SHA256 sidecar, and uses a --release --locked --target build without dev-fixtures. This separates production packaging from earlier fixture-enabled smoke. It is a local package build, not deployment. The source-package SBOM gathers Cargo metadata; no security audit or vulnerability verdict is inferred. The final workflow filename regex inspects archive entry names only, not file content or executable feature behavior. The checksum detects mismatch against its sidecar, not semantic validity when both bytes and checksum agree.

## Actual bounded archive-gate counterexample

Before proposing any workflow fix, the worker executed the exact captured lines76–77 using real bash-e, shasum, tar and grep on three temporary local fixtures. No release build/script, GitHub runner, Linux execution, real credentials or shared-main mutation was involved. HOME/CODEX_HOME were preserved. Results:

| Actual input | Checksum | Independent tar listing | Exact final gate exit |
| --- | --- | --- | --- |
|Valid archive with harmless README|OK|0|0, accepted|
|Valid archive with a harmless-byte file named .env|OK|0|1, rejected|
|Malformed archive with a checksum matching those malformed bytes|OK|nonzero/error|0, incorrectly accepted|

Successful probe receipt/log is worker-owner-review/archive-gate-counterexample.json/.log, log SHA256 `f68260af1af39bd62d1b99861296dd21ac062450ad11869ef892edbc8bb1d407`. It records actual GNUbash3.2 and BSDtar tool versions on local macOS. Three actual cases demonstrate the shell gate's pass/fail behavior; they are not three hosted CI passes. The malformed archive causes tar to fail and produce no names; grep has no match and exits1, so `!` returns success. This is a confirmed validation defect in the exact gate, not a claim that current release.sh normally generates malformed archives. Main was sent the counterexample/log and proposed owned correction path .github/workflows/ci.yml before any product work; this package changes only report/evidence and includes no correction. A future fix should require successful complete archive listing separately from forbidden-name rejection, then preserve both positive and negative cases; main owns that decision.

## Stage1 coverage, source mapping and evidence limits

The unchanged workflow explicitly calls v1/v2 validators but contains no validate_stage1_blueprint.py, G-STAGE--item or stage1_host_smoke invocation. It therefore does not directly enforce this current blueprint's per-item evidence/master-receipt gates through a named CI step. Cargo can run stage1 Rust tests through the all-targets invocation, but Rust test success is not the same as independently accepted learning reports or stage1 semantic receipts. The workflow's older contract naming must not be relabeled current-stage acceptance. It also does not explicitly run Python unit suites or an MSRV/macOS/Windows/default-only feature test matrix. These are exact scope observations, not claims those checks fail elsewhere.

Existing local115 native-fixture/pipe packages have real Rust/productionJSONL/lint/budget evidence under their own toolchains and scopes. They are referenced only as local evidence, not as a hosted Ubuntu run of this77-line workflow. The budget implementation's eight limits are16normaldependencies/8MiBrelease/1000mscoldmax/96MiBRSS/2000usqueue/10000usrender/100uslayout/onecoalescedframe; those constants were read from the called script, and historical measurements are not rerun here. The new archive probe is the only089 behavior execution. No workflow run is triggered, no GitHub artifact is uploaded, and no dangerous release-script cleanup command is executed during review.

Pi-mono source test/runtime reports supply behavior comparisons for target owners, not proof that this GitHub Actions workflow covers every source case. ZS1-092 workflow-directory integration remains separate; other file/root acceptance is not inherited. Main's116 status and future122 changes are context only. All77lines and every step are documented; the file is not substituted with a directory summary.

G-FILE requires main's independent semantic review. G-STAGE is run separately against read-only main with bytecode writes disabled; missing master receipt/structural validity is not acceptance. Evidence verification covers current=original hash, full77-line interval, sixteenorderedsteps, copied script context/reference hashes and actual probe receipt. Candidate adds this report and089evidence only; rollback removes them and restores no product file because none was changed. No commits, newtasks/subagents, sharedroot edits or acceptance promotion.


---

# 3.1.20 独立完整复核：ZS1-089

本节是当前结论；上方12050字节旧报告原样保留，属于3.1.16时点，不能将其中“current=77行”“未修复归档缺陷”“唯一089行为运行”等描述套用到本轮。主库本报告接收基线仍为 absent，本地旧前缀 SHA256 `60ecb71dd2898ae62ed20169442f4ec36a58894dfb380d50fea1ed2fd965f96e`。本轮候选状态 `[_]`，无主控验收替代。

## 正式范围与完整阅读

本轮先核对主库 `Docs/stage_1_v3_pi_mono_blueprint.md` 3.1.20：ZS1-089正式对象仅 `.github/workflows/ci.yml`，layer L1，依赖ZS1-001，Owned path为此报告；验证器G-FILE、G-STAGE --item ZS1-089；回退仅本报告/本项状态，不覆盖源码。ZS1-092/093目录整合独立。requirement digest `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`。

先连续全文读取77行冻结基线，再连续全文读取94行当前文件，包括所有空行、注释、表达式、16个步骤和多行shell的全部分支。基线2480B SHA256 `5d7173fa20a0de949e53525018dad07718a9e4a9d24a5fcd137b056af31792d6`；当前3104B SHA256 `b9e70ba448d157341e0069fcd9ad19eae8f0412aaf0b9e03bbd97b4f2ef59dbd`。冻结全文件、单连续字节区间、时间及原路径见worker-review320/input-manifest.json；完整有序step-index是阅读后的辅助核对，不替代正文理解。基线前76行与当前前76行相同，最后旧pipeline被18行显式控制流替换。

本轮只写此报告和独立证据，保留65份现有ready manifest及旧候选全部字节。没有产品修改、主库写入、Cargo/PTY/HTTP、发布、git提交或验收勾选；新增真实行为运行数0。无需读取正在变动的TUI。必要调用脚本按捕获时间与整文件hash绑定片段，明确未接受整兄弟owner。

## 当前文件逐字段、逐步骤语义

L1–5名称与push/pull_request触发，无branch/path过滤、schedule、手动入口；L7–8只声明contents:read；L10–12按workflow/ref分组并cancel-in-progress，这是Actions运行并发控制，不代表zenpi会话取消或锁。L14–19仅一个verify job、ubuntu-latest、15分钟，无OS/toolchain/feature matrix、工作目录或shell override。四个外部action引用均为版本标签/分支，没有完整commit SHA；runner latest和Rust stable也非冻结身份。没有从PR标题等非可信表达式拼接到run shell；github.workflow/ref只用于concurrency，run_id只用于artifact名。checkout未覆盖默认inputs。这里只描述声明，不声称平台运行时权限、缓存来源或外部action当前实现已独立审查。

| 当前行 | 步骤 | 输入、作用及失败边界 |
| --- | --- | --- |
|20–21|Check out source|actions/checkout@v4，无附加with；不操作本次worker主库。|
|23–26|Install stable Rust|dtolnay/rust-toolchain@stable，components为rustfmt、clippy；不提供MSRV版本矩阵。|
|28–29|Cache Cargo artifacts|Swatinem/rust-cache@v2，无本文件级key覆盖；缓存命中不是正确性验收。|
|31–32|Check formatting|cargo fmt --all -- --check，检查而非改写。|
|34–35|Run Clippy|cargo clippy --all-targets --all-features -- -D warnings；warnings失败，无显式--locked。|
|37–41|Run tests|cargo test --all-targets --all-features --locked -- --test-threads=1；注释为loopback资源争用而串行libtest，不能解释成内部不创建并发或包含ignored用例。|
|43–44|Validate execution blueprint and Gantt|python3 tools/validate_blueprint.py；当前入口常量指向旧Zenpi_Execution_Blueprint/Spec/Gantt，不是stage1 checker。|
|46–47|Validate v2 blueprint review draft|python3 tools/validate_blueprint_v2.py，默认非权威v2 review draft。|
|49–50|Report Rust source inventory|python3 tools/check_rust_loc.py，默认informational且未传--max-lines；不能写成LOC硬拒绝。|
|52–53|Run runtime and size budget gate|bench_runtime.py --samples 3 --output /tmp/zenpi-runtime-budget.json；无--no-fail，当前main先采集/测量，最终写JSON后按8个gate返回；前期异常可能没有汇总文件。|
|55–59|Upload runtime budget receipt|upload-artifact@v4，仅上传该/tmp汇总JSON；name含run_id，没有failure/always条件或startup evidence目录。|
|61–62|Check two-mode boundary|check_modes.py检查RunMode与帮助，然后执行默认cargo build；当前main全文读过53行。它限制public mode，并不禁止其它管理CLI。|
|64–65|Exercise headless JSONL protocol|fixture-enabled debug build成功后才运行指定ZENPI_BIN的headless_smoke.sh；本行无--locked，不传allow-skip。|
|67–68|Exercise the installed release user paths|ZENPI_SMOKE_FEATURES=dev-fixtures user_smoke.py；当前main先生产release build/install与生产入口检查，再fixture build/install与echo/TUI检查，不能因环境变量把整步说成仅fixture。|
|70–71|Exercise the release headless protocol|fixture-enabled release build成功后运行指定binary的--release JSONL smoke；不是最终package binary行为证明。|
|73–94|Package and verify the production artifact|release.sh→dist sidecars校验→mktemp→逐archive成功listing→拒绝禁止名称或grep异常；下面逐分支说明。|

流程中没有continue-on-error；正常步骤的success依赖和默认shell语义此前已按GitHub官方文档核对，原访问记录2026-09-10保留在旧089包external-reference.json，本轮未联网重新验证。这里不把本地bash回放写成GitHub托管执行。job的15分钟以及cancel-in-progress可能打断后续步骤，但源码本身无法证明运行耗时是否会超限，也不能从取消声明推断产品子进程已reap。

## 当前归档检查完整分支与已有修复

L75运行release.sh，L76在dist校验匹配sidecar。L77取得临时清单路径，L78注册EXIT清理。L79遍历每个dist/*.tar.gz；L80先完整执行tar listing到清单，L81–83在失败分支打印archive来源并exit1，不再借grep状态掩盖tar错误。L84–86有禁止名称match则失败；L87–92无match分支捕获grep的实际退出码，只有1视为普通未匹配，0在前面拒绝，其它非零保留错误码失败。L93–94闭合条件和循环。路径变量均引用，grep模式与基线相同；新增逻辑不解压、不执行归档内容。各archive先清空同一个临时清单再检查，不将多archive交给单个tar参数解释。

这是当前已实现的修复，不应继续提“修复 !tar pipeline”为新缺陷。已有历史修复包stage117-ci-archive-ready的candidate-ci.yml与本轮current逐字节相等，基线亦完全绑定。两份历史probe源码已全文读完：旧089 probe22行，117 replay55行；只摘出checksum/listing shell，明确排除release.sh，不调用产品。前者本地构造三个case；后者对每case给旧/新两个shell相同archive字节，核验checksum成功、独立tar状态、临时清单清理，并保留base64及hash。

| 相同历史输入 | 旧归档gate | 当前归档gate | 实际含义 |
| --- | --- | --- | --- |
|有效README归档|0|0|允许正常名称|
|含无害测试字节.env条目|1|1|拒绝禁止名称|
|checksum相符但格式损坏|0|1|旧误通过已转为显式tar失败|

117 three-case-replay实际wrapper退出0，日志hash `58d1f5f757016f2e2ef9c9befc618984670d0baf7f4ddfcf832ab00e84d76426`；0指断言了预期正负结果，不是六次gate都成功。原089反例日志 `f68260af1af39bd62d1b99861296dd21ac062450ad11869ef892edbc8bb1d407`继续保留。历史candidate-shell-syntax实际0只对应抽出的shell语法，不能替代完整Actions YAML语义验证。

历史工具为macOS arm64 bash3.2/BSD tar，未提供托管Ubuntu实际run。多个归档、缺失归档、grep读取错误、取消中清理没有增加成历史已执行case；它们仅有控制流解释。名称regex只查列出的路径字符串，不查内容、symlink目标、文件类型或二进制feature。校验和只能保证与sidecar一致，不能证明来源真实性。release.sh实际stage为binary、README、LICENSE、SBOM，默认无dev-fixtures的--release --locked --target构建；无部署/上传。上述限制是此门禁职责，不凭假想恶意归档将其写成远程执行漏洞。

## 当前调用方更新与仍需处理的差距

调用引用均冻结整文件身份，但只保存必要内容：validate_blueprint/v2与LOC是入口片段；bench_runtime是预算常量和main片段；headless_smoke是参数/build选择片段；user_smoke是isolated_env与main片段；check_modes/release是小文件全文。未全文审查user_smoke各测试helper，也未读取全部Cargo测试；本文件本身无inline tests。没有将兄弟脚本全文hash转换为兄弟文件验收。

1. **P2，预算失败后的诊断留存未闭合。** CI的Upload紧跟budget且无显式失败条件，按既有平台默认success规则，budget返回1后这一步不会正常保留结果。当前bench_runtime还把逐样本stdout/stderr/身份写到新的.ops/runtime-budget时间目录，而workflow只上传/tmp汇总；汇总有样本内容/摘要不能替代保留所有独立诊断文件。建议后续只做这个具体改动：为有界预算产物设置失败可达上传，保留真实budget非零，并清晰区分未生成文件与测量失败。本轮未运行失败CI验证，当前源码可见缺口与以前相同。

2. **P2，当前Stage1验收未由本workflow直接执行。** 步骤明确运行旧v1/v2 checker，没有validate_stage1_blueprint.py、G-STAGE --item或报告master receipt验证入口。Rust all-targets可能覆盖Stage1实现测试，但不等同于学习文档/语义receipt验收。建议由主控决定在已确定的权威范围内添加结构门禁及必要生产入口回归，不用一个CI绿灯概括整个蓝图。此条是接线缺口，不声称其它自动化都没检查。

3. **P2，归档修复回归只留在历史证据包。** 当前workflow直接执行正常release packaging，没有显式运行上述坏归档/禁止名称before-after回归脚本。源码已修复，历史三个case已通过预期断言，但后续改坏分支可能缺少持续回归覆盖。建议把有限归档门禁case放入维护的测试入口，并在Ubuntu实际CI记录其正负结果；本轮不另行运行/迁移测试。

4. **版本身份与验证范围限制。** Actions major/stable标签、ubuntu-latest与stable toolchain浮动，文件不能给出精确执行依赖身份；可后续固定外部action提交并由维护流程更新。Clippy/debug/release smoke行没有统一--locked，而test/budget/release.sh/user_smoke多处有锁文件约束。没有OS/MSRV/feature组合测试矩阵，不代表生产default从未测试：当前user_smoke明确新增了production install/provider/tool approval/shell/EOF/GC/blueprint入口。不要沿用旧报告遗漏该部分的概括。

5. **工作环境边界。** 当前user_smoke isolated_env仍移除CODEX_HOME并设置child HOME。它是该脚本隔离fixture环境的现状，不应替用户决定把此模式推广到本轮工作；本轮没有执行它，真实HOME/CODEX_HOME保持。仅凭这个helper不判定产品安全缺陷。shell脚本的STAGE删除是release脚本运行语义，本次只读，不执行release cleanup。

## 完整性、接收和回退

旧089候选18个member、旧117修复候选22个member连同两个manifest完整复制到本项reused证据，逐文件校验，保留历史authority/日志/失败案例而不篡改到3.1.20。旧manifest分别为`e4584db1d96af2d80ad3e8a660daa3a47c69f244b0998526f5efcb10e8605d47`、`160a2115a98abc13ba7cd022f2aad28dff583b5e8573c867393799e8fb409d37`。脚本只做字节/结构验证，不重跑历史probe，不构建产品、不触发Actions。

本项独立checker验证authority、主库确切路径、冻结77/94全文覆盖、16步骤顺序、diff、相同旧前缀、历史manifest40个成员、base64字节、真实receipt/log hash、调用方捕获身份、65旧ready和未改working CI。frozen模式可在封包files目录脱离live主库验证。G-STAGE原始exit/log单独记录；主master receipt缺失时保留非零和`[_]`，不改状态。接收补丁以主库absent报告为基线携带完整旧前缀+本节及全新证据；不是把主库文件force覆盖。所有正逆apply/check在临时目录进行，回退只删除本次新路径。候选完成后交主控，ZS1-092/093或其它owner均未接收。


---

# 3.1.20 当前 CI 差量复核：113 行预算证据保留版本

本节更新当前结论。此前完整 24526 字节报告作为精确前缀保留，SHA-256 `33e353712730db9dcb9335e8c16c8d9caa83c2488a0d7b851d3042a33dfa670f`；其中 77 行与 94 行的“当前”描述分别属于历史时点。特别是上一节第 1 条“预算失败后的诊断留存未闭合”已由主控整合的当前代码改变，不能再作为当前未修复缺陷。此刷新候选仍为 `[_]`，未修改状态、产品或目录报告。

## 范围、输入和完整阅读

当前蓝图 ZS1-089 的 L1 owner 仅 `.github/workflows/ci.yml`，依赖 ZS1-001；唯一 Owned path 是 `Docs/learn/stage1_pi_mono/targets/zenpi/files/.github/workflows/ci.yml_learn.md`。G-FILE 要求完整语义与分支复核，结构 checker 不能替代主控语义验收；G-STAGE 的 `--item ZS1-089` 另行检查。ZS1-092→093 依赖和目录闭包仍独立，不在此报告接受。

本轮连续全文读取冻结基线 77 行、上一候选 94 行和主库当前 113 行，完整覆盖空行、注释、字段、16 个有序步骤及 shell 分支；没有用 diff 代替全文。输入的单连续字节范围、捕获 UTC、原路径和摘要见 `Docs/quality/stage1/ZS1-089/worker-current-ci320/input-manifest.json`。

| 版本 | 字节 / 行 | SHA-256 |
|---|---:|---|
| 蓝图冻结基线 | 2480 / 77 | `5d7173fa20a0de949e53525018dad07718a9e4a9d24a5fcd137b056af31792d6` |
| 上轮完整理解候选 | 3104 / 94 | `b9e70ba448d157341e0069fcd9ad19eae8f0412aaf0b9e03bbd97b4f2ef59dbd` |
| 当前主库 | 3973 / 113 | `44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9` |

requirement digest 保持 `8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645`。新证据目录的 `reused/` 保存上轮完整 089 ready、预算变更 worker ready、主控 `master-ci-budget-evidence-3.1.20` 全部常规文件；包括原 manifest、失败结果、日志及回放源码，不改其历史 authority 或执行时间。继承报告的 master 接收基线仍不存在；本轮同时提供从 absent 主库接收的完整标准报告补丁，以及从旧候选精确前缀追加的补丁。

## 当前全文件接线与逐步骤复核

L1–19 保持原样：push/pull_request 无过滤，contents:read，workflow/ref 并发组和 cancel-in-progress，一个 Ubuntu latest verify job、15 分钟超时。新增 shell:bash 只作用于预算步骤；原报告“无 shell override”不能继续概括当前整个文件。权限声明没有变宽，也没有新增部署、消息发送或发布步骤。contents:read 是工作流声明，不是远程 artifact 服务调用成功的证据；实际令牌、action 内部实现和运行器权限不在本轮验证内。

| 当前行 | 实际入口与分支范围 |
|---|---|
|20–21|checkout@v4，无输入覆盖。|
|23–26|rust-toolchain@stable 请求 rustfmt、clippy，无 MSRV 矩阵。|
|28–29|rust-cache@v2，缓存不构成行为验收。|
|31–32|cargo fmt --all -- --check，只检查。|
|34–35|Clippy all-targets/all-features，-D warnings，无显式 --locked。|
|37–41|测试 all-targets/all-features/locked，libtest threads=1；保留解释 loopback 争用的注释。|
|43–44|validate_blueprint.py，命令未变；此前检查的旧蓝图入口身份属于历史上下文。|
|46–47|validate_blueprint_v2.py，命令未变；没有增加当前 Stage 1 checker。|
|49–50|check_rust_loc.py，无新增 --max-lines 参数。|
|52–70|runtime_budget 显式 Bash、独立证据目录、samples=3，捕获两个管道成员退出码后显式返回。完整分支见下。|
|72–78|always 与 budget outcome 条件，upload-artifact@v4 上传目录，名称带 run/attempt，缺文件为 error。|
|80–81|check_modes.py，命令与上轮相同。|
|83–84|fixture debug build 成功后通过 && 执行指定 ZENPI_BIN 的 headless_smoke。|
|86–87|dev-fixtures 环境参数调用 user_smoke.py；本轮不扩展到脚本内部 helper 验收。|
|89–90|fixture release build 成功后运行指定二进制的 --release headless smoke。|
|92–113|生产打包、校验 sidecar、临时清单、逐归档显式检查 tar 成功、拒绝禁止名称及 grep 异常。|

94→113 的差量精确局限于第 10、11 步（零基下标 9、10）；前 9 步与后 5 步的原始文本均逐字节保持。归档检查仅移动了行号：当前 L94–95 为 release/校验和，L96–97 临时文件与 EXIT 清理，L98–102 listing 失败直接退出 1，L103–105 禁止名称匹配退出 1，L106–111 仅 grep=1 视为普通未匹配，其余非零保留错误码，L112–113 闭合。旧 malformed archive 误通过的修复仍在，不能重新提为当前 bug。其历史有效/禁止名称/损坏归档证据保持原样；本轮没有增加归档行为案例。

## 预算 shell 的完整失败、持久化和取消边界

1. L53 的 id 供随后 `steps.runtime_budget.outcome` 使用。L54 明确选 Bash；本地历史夹具实际用 `bash --noprofile --norc -e -o pipefail` 执行抽出的 run 块。平台默认 shell 的依据沿用历史官方引用，本轮无网络查阅或远程运行。
2. L56 通过 env 传入 runner.temp 下由 run_id/run_attempt 组成的目录；不是在 run 字符串中插入 PR 标题、分支正文等用户输入。所有 shell 路径展开均带双引号。L58 使用普通 mkdir，位于 set +e 之前，创建失败会使步骤失败；不加 -p 是拒绝重用已有目录的控制流。该目录身份按 workflow run/attempt 区分，不是跨任意未来 matrix/job 的通用唯一标识；当前只有单个 verify job。
3. L59 暂停 errexit，但不撤销 pipefail，使预算或 tee 非零后仍能执行状态保存。L60–63 固定 samples=3，向 summary.json 与尚未创建的 startup 子目录传参；没有 --no-fail、--near-cap、阈值修改或重试。`2>&1` 合并预算 stdout/stderr，再由 tee 写 budget.log 和步骤输出；预算脚本自身的异常文本也可进入此管道。
4. L64 紧接管道，以数组展开复制 PIPESTATUS，索引 0 为 Python、1 为 tee；没有先执行其它命令破坏该管道状态。L65 再恢复 errexit。L66 的 exit-code.txt **只记录预算进程退出码**，不是最终步骤退出码，也没有单独持久化 tee 的退出码文件。预算=0、tee=23 时文件内容为 `0\n`，步骤仍失败。
5. L67–69 预算非零优先原码退出；只有预算为零才走 L70 返回 tee 状态。因此两个成员同时失败时预算码优先；logger 失败不会将步骤变绿。这个保证适用于执行确实到达这些分支的情况：exit-code.txt 的重定向或 printf 在恢复 -e 后失败，会提前结束步骤，不能绝对承诺任何 I/O 故障下都保留预算原码。
6. `tools/bench_runtime.py` 当前整文件摘要仍为 `ef6701f52659c78f58fb3f31adf87343c31b34229145f57acba2be05b1a9a52c`。本轮仅复核带 hash 的 L1–58、L142–201、L276–338 上下文：CLI 拒绝已有 startup 目录后创建；startup 成功和捕获异常路径写 sample-NN.json，单样本以 x 模式拒绝覆盖；summary 经临时文件与 os.replace 写入后再返回 gate 结果。dependency/build 等早期异常可能发生在样本或汇总生成前，不能承诺总有 3 个样本或 summary。
7. startup 观察记录将原始子进程流转为全流 bytes/hash 与最多 65536 字节 base64 前缀，另有 truncated 标记。因此“保留启动诊断”表示保留已有诊断格式，不能误写为所有子进程完整原始流都会独立上传。budget.log 保存的是本次 Python 命令向其 stdout/stderr 实际发出的合并字节。
8. 超时、并发取消或 runner 被终止可能打断状态写入、样本写入及上传。工作流没有为预算步骤增加自动恢复、重新测量或会话恢复合同；run_attempt 区分重跑目录，不意味着预算脚本会恢复未完成采样。本轮不把 workflow 取消声明替代产品子进程取消/reap 验收。

## 上传条件、失败传播及适用限制

L73 含状态函数 always()，并排除 outcome=`skipped`。预算步骤成功、失败，以及平台仍能继续调度的取消路径都可能进入上传；因较早普通步骤失败而预算被标为 skipped 时不会上传。该表达式使失败证据上传可达，但无法保证已丢失的 runner、作业强制超时或无法继续调度时仍能完成 action。

L76 名称和 L77 路径均包含同一 run_id/run_attempt；上传范围改为整个证据目录，可能包含 summary.json、budget.log、exit-code.txt、startup/*.json，具体取决于生成进度。L78 的 if-no-files-found:error 在没有可上传文件时要求 action 报错，不代表只有 summary 缺失就应报错；早期失败留下的日志和部分诊断仍有保留价值。没有 continue-on-error：若预算本来成功而上传失败，后续普通成功条件步骤会被阻断；若预算已失败，上传成功也不会把原失败改为成功。这里解释配置/已有文档语义，未证明任何实际远程上传已经成功或失败。

普通 mkdir 拒绝旧目录可避免预算覆盖它，但上传步骤没有额外检查“此次 mkdir 成功”的输出。如果人为复用同一 run/attempt 路径造成 mkdir 失败，配置仍可能尝试上传该已存在目录；本地重复目录夹具只证明拒绝覆盖和不再次调用预算，未执行上传。正常唯一 run/attempt 命名和“任何失败时 artifact 都一定属于本次执行”是不同强度的保证，报告不将后者补造为已证事实。

contents:read 及其他 14 步未改；本轮没有对 action 服务端授权、artifact 保留期、容量限制、不可用/断网重试、Ubuntu 文件系统错误进行运行验证。action 版本标签与 ubuntu-latest/stable 的浮动身份限制仍适用。

## 五种历史本地 shell 证据与本轮运行数

新证据目录内 worker 历史 `local-fixtures.json/.log` 绑定 2026-09-12T11:59:00Z 执行，wrapper exit 0，log SHA-256 `39a9062ef01e8415b2ed21f85852c6a5738c287828da820eb9542a3524b117ca`。主控历史 `shell-cases.run.json/.log` 绑定 2026-09-12T12:22:12Z 独立复跑，wrapper exit 0，log SHA-256 `51e6a914276eec1210cc17d4298328b8fd60b2f25aee13987551d02c4b6a4281`。两者为同一五种场景的不同执行，不计作十种场景，也不是本轮新运行。

| 场景 | 预算 / tee / shell 实际码 | 字节与生成预期 |
|---|---|---|
|success|0 / 0 / 0|合并 CRLF 日志、summary、startup 样本及 exit-code=0|
|budget-failed|1 / 0 / 1|失败 summary、日志、样本及 exit-code=1|
|early-failed|17 / 0 / 17|日志、部分样本及 exit-code=17；没有伪造 summary|
|logger-failed|0 / 23 / 23|exit-code=0 但 shell=23，明确区分两个身份|
|both-failed|17 / 23 / 17|预算原码优先，保留部分样本和日志|

两次历史验证均逐例断言中文/空格路径、stdout/stderr 合并后的 CRLF 精确字节、startup 初始不存在、samples=3、同目录再次执行时 mkdir 拒绝、fixture 调用总数仍为 1、旧证据与无关 sentinel 不变。logger=23 是包装系统 tee 写完后主动返回 23，证明退出码传播，不是磁盘满、断管或真实文件权限错误注入；其日志完整不能推广到所有真实 tee 故障。历史 verifier 还进行了单 CI 文件正逆 apply/check；本轮保留源码和全部原结果，但不执行它。

本轮新增行为运行数 **0**；Cargo、PTY、HTTP、网络、生产 budget、Actions 和实际 artifact 上传均为 **0**。本轮实际运行的是只读 G-STAGE 与新包离线完整性/标准报告补丁校验，不能计入上述五种 shell 场景。未复用其它项的通过数量或主控 125/TUI 验收作为 089 的行为证明。

## 仍开放的范围与交付门禁

预算失败上传路径的具体修复已落到当前 owner；远程 Actions/真实上传仍缺运行证据。上一报告的当前 Stage 1 checker 接线、归档坏输入持续回归入口、浮动依赖身份和验证矩阵限制，没有被本次两步骤差量解决；本轮只确认当前 YAML 命令未新增这些入口，不宣称其他自动化没有覆盖。旧报告对 user_smoke 等兄弟脚本的 helper 解释保持历史时点，未借本轮 hash 重新验收那些文件。

本轮实际只读 G-STAGE 返回 1：structural.ok=true，semantic_manual.verified_by_checker=false，缺失主库 `Docs/learn/stage1_pi_mono/receipts/ZS1-089.master.json`。原始命令与日志保存于 commands/gstage-current-main，日志 SHA-256 `fd4e98ca2d279a3a8f6df7fe7ca1b6f6e56a655d5976318aee2f2d673e90638f`。这是未具备主控验收 receipt，不能隐藏非零、补造 receipt 或把离线结构校验当作 G-FILE 接受。

新 ready 的 manifest 逐文件记录完整 payload 大小与摘要；离线 verifier 校验旧报告精确前缀、77/94/113 行完整范围、16 步顺序、两步骤差量、历史 manifest/原始日志/base64 和标准报告两个基线的正逆补丁。补丁只写唯一标准报告，历史证据在独立目录随包保留；回退只撤回该报告候选和本轮新证据，不覆盖 CI 产品、主库状态或 092/093 报告。主控应在精确输入仍匹配时独立验收，再推进目录项。

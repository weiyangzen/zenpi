# ZS1-065 . 独立目录整合报告 [_]

Worker（受限 claim `ZS1-065-20260917T030519-121`，layer `L2`，run `20260917T030519-121`，baseline `58a6f2accff21a188849da713b80fcc8b8024e71`）。本候选只审阅 pi-mono 仓库根目录 `.` 的**目录整合关系**：直属文件与直接子目录的集成。正式直属 in-scope 子项恰为 `ZS1-064 packages`（唯一冻结子树），root 无直属冻结源文件；`.github`、`.husky`、`.pi`、`Docs`、`scripts` 五个直接目录及全部根配置文件均为 context-only。任何目标 owner 与任何产品项**均不由本候选验收**。worker 自测只到 `[_]`，`[x]` 只能由 master 集成行为证据产生。

## 权威与源冻结绑定

- 当前权威为 `Docs/stage_1_v3_pi_mono_blueprint.md`（`blueprint_version 3.1.22`、`authoritative:true`）。本 work tree 该文件实测 SHA256 = `7ff9cbe676dce83567a8b4dc43a1861b1c8f03075d95d1c7fb4c9da082e4f59d`，与本 claim 的 `blueprint_digest` 逐字一致。
- 本机 validator 组件 `parse()` 对本 work tree 权威全文复算 `requirement_digest = f69d1f67dc6a8238f891ca8583071c72397891a3adaa17169a6d9e8516b7bc76`、`snapshot_sha256 = 7ff9cbe676dce83567a8b4dc43a1861b1c8f03075d95d1c7fb4c9da082e4f59d`，与 sibling `ZS1-062/063/064` 记录一致（三者 requirement_digest 均为 `f69d1f67…`）。
- 第 31–37 行 `源冻结重签声明`：迁移前 `bbb61e34aaf231639fdaaad1adbd757947034eac` 在 origin 及本机均不可达；本 run 以 `source_revision 23282f60782f02b9e22b787e4b22af441454fa16` 重冻结；第 3 节源/目标行 SHA256（含可用字节数列）为**权威源指纹**；第 36 行明确 `ZS1-012/013/016/017/024/029` 源路径在当前 revision 已不存在，声明为历史报告一致性绑定，**缺源不构成 blocked**；历史报告内部旧 revision hash 一律被本声明取代。
- 本机 `source_repo`（`/Users/mac/GitHub/pi-mono`）HEAD 实测 = `23282f60782f02b9e22b787e4b22af441454fa16`，与蓝图 header `source_revision` 一致（`git status --short` 为空，clean，无未提交改动）。

## 真实直接清单与阅读边界

`.` 为 **14 个 tracked 直属文件、6 个 tracked 直接目录**（`git ls-files | awk -F/ 'NF==1'`）。另有未跟踪的 `.DS_Store`（被 `.gitignore` 忽略），不计入。逐字节/SHA256 为本机只读 `source_repo` work tree 实测；递归计数含全部子目录文件，仅用于范围说明，**不构成验收**。下层只清点，不递归接受子树。

| 直接项 | 类型 | 字节 | SHA256 | 范围与职责 |
| --- | --- | ---: | --- | --- |
| `package.json` | 文件 | 3184 | `47c9a8b5b1487fbe4b5a169c5c5f9131fd42e57dff69300b8f183abb59dcab4c` | context-only；根 workspace 定义 |
| `package-lock.json` | 文件 | 285909 | `ed9fca979f6705516b2e5cd0f537d817f2ffe14f76db572595d351155daa671e` | context-only；锁文件 |
| `tsconfig.json` | 文件 | 1547 | `e733d81a7c5f8c6eab28e779bfdfb0208cec8b6ff244515442874a84f9c7ff05` | context-only；根 TS project + paths |
| `tsconfig.base.json` | 文件 | 553 | `478a425fe9f8f39dee52d1e3485e612e892113278e8f4430545600ad30a01bac` | context-only；编译基线 |
| `biome.json` | 文件 | 895 | `63eb6ad3e4022985a47f9105facae1e81e132709bf495d0e547396dd8356c2ad` | context-only；lint/format |
| `README.md` | 文件 | 2754 | `c47342a4051a9f00c5b9b7327faa167869db9abe9068c013f40a3e26559ef087` | context-only；仓库说明 |
| `AGENTS.md` | 文件 | 10893 | `f7fb37c3fcee0054ba250d6fb79f239543102d0633be8b35d305db6afcc47002` | context-only；项目规则 |
| `CONTRIBUTING.md` | 文件 | 1685 | `977f3ad7b9b39c45d7533426e318e9555768d5dc0f2fec5881550858d08335b3` | context-only |
| `LICENSE` | 文件 | 1069 | `0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48` | context-only |
| `.gitattributes` | 文件 | 382 | `df53a67c845d456ef77199ebb74762cd294bb8f84492aa744820d4223df2abf8` | context-only |
| `.gitignore` | 文件 | 424 | `be21cab8cb6ecd8fce59b89234ae9f06e4960333ccfeddeb981f5819e1db0410` | context-only |
| `pi-mono.code-workspace` | 文件 | 115 | `37afd07fd955745316e8e2f32e340df4198cf61858073903e05e045c06340259` | context-only；VS Code workspace |
| `pi-test.sh` | 文件 | 1400 | `2241689dbc44d37aac27613e2e5152e869e70df94165d27d53bd594e6a08a5ce` | context-only；源运行脚本 |
| `test.sh` | 文件 | 1474 | `2e7dc3f01816167615384f79bac5896d357cc9fc23ac8a920d9dbefb68e76dee` | context-only；测试入口 |
| `.github/` | 目录 | — | — | context-only；3 文件+2 目录，5 个 workflow |
| `.husky/` | 目录 | — | — | context-only；1 文件（`pre-commit`） |
| `.pi/` | 目录 | — | — | context-only；4 直接目录（extensions/git/npm/prompts），11 文件 |
| `Docs/` | 目录 | — | — | context-only；1 直接目录 `researches/`，117 目录/822 文件，**不在冻结子集** |
| `packages/` | 目录 | — | — | **正式 `ZS1-064`（本 work tree 蓝图 `[x]`）**；7 直接目录，103 目录/691 文件 |
| `scripts/` | 目录 | — | — | context-only；10 个直属文件 |

六个直接目录均**无符号链接**（`find <dir> -type l` 均为 0）。root 无任何直属冻结源文件；§3 的 21 个 pi-mono 源文件全部位于 `packages/` 子树内。

## 冻结子集与直接子目录闭包

root 的冻结 in-scope 子项只有 `ZS1-064 packages`。validator `parse()` 对 source scope 计算 root 的期望直接子目录集合 = 全部源文件的 parent 集合 = `{., packages, packages/agent, packages/agent/src, packages/agent/src/harness, packages/agent/src/harness/compaction, packages/agent/src/harness/session, packages/agent/test, packages/ai, packages/ai/src, packages/ai/src/api, packages/coding-agent, packages/coding-agent/src, packages/coding-agent/src/core, packages/coding-agent/src/core/extensions, packages/coding-agent/src/core/tools}`（16 个，对应 `ZS1-050…ZS1-065`）。

| 目录项 | folder | Depends（=直接 in-scope 子项） | 状态 |
| --- | --- | --- | --- |
| `ZS1-064` | `packages` | `ZS1-061,ZS1-062,ZS1-063` | `[x]`（本 work tree 蓝图；canonical scaffold 仍显 `[ ]`，见替代记录） |
| `ZS1-065` | `.` | `ZS1-064` | `[_]`（本报告） |

- **G-DIR 闭包（组件级）**：`ZS1-065` 的直接 in-scope 子项 = 直属冻结文件（无）∪ 直接冻结子目录 = `{ZS1-064}`，与行 `Depends: ZS1-064` **逐字相等**；无缺项、无多余、无跳目录（不直接接受 `packages/agent` 等下级）、无重复接受。
- **in-scope**：`ZS1-064` 及其递归冻结后代。`ZS1-064` 已把 `ZS1-061/062/063` 三个子包整合为 `packages` 闭包，本层只接受该唯一直接子目录。
- **context-only**：`.github`、`.husky`、`.pi`、`Docs`、`scripts` 五个直接目录，全部 14 个直属文件，以及 `.DS_Store`。它们被读取只为集成理解，不计入完成率、不形成目标义务。

## 跨目录集成关系（本目录层新增的唯一分析）

- **workspace 容器**：根 `package.json`（`pi-monorepo`，`private`，`type:module`）以 `workspaces` 声明 1 个主 glob `packages/*` 及 4 个嵌套示例 workspace（`packages/web-ui/example`、`packages/coding-agent/examples/extensions/{with-deps,custom-provider-anthropic,custom-provider-gitlab-duo,custom-provider-qwen-cli}`）。冻结子集仅是 `packages/*` 下的 3 个子包，`mom`/`pods`/`tui`/`web-ui` 为 context-only。
- **构建顺序**（根 `build`）：`tui → ai → agent → coding-agent → mom → web-ui → pods`，依赖产物先行；`dev` 并发 `ai/agent/coding-agent/mom/web-ui/tui`。冻结包依赖边见 `ZS1-064`：`agent → ai`、`coding-agent → agent/ai/tui`。
- **类型路径**（根 `tsconfig.json`）：以 `paths` 把 `@mariozechner/pi-*` 指向 `packages/*/src`，并含 `pi-coding-agent/hooks → packages/coding-agent/src/core/hooks/index.ts`（该目录在当前 revision 不存在，`ZS1-063` 已记为 stale 源事实）与 `pi-agent-old`（`packages/agent-old/src`，不在冻结子集）。`include` 覆盖 `packages/*/src|test/**` 与 `packages/coding-agent/examples/**`；`exclude` 为 `packages/web-ui/**`、`**/dist/**`。
- **根 CI**（`.github/workflows/ci.yml`）：`npm ci → npm run build → npm run check → npm test`（Node 22，装 `fd`/`ripgrep`）。这与 `npm run check`（`biome check` + `tsgo --noEmit` + browser smoke + web-ui check，README 注明须先 build）共同构成跨目录集成合同；其余 4 个 workflow（`approve-contributor`、`build-binaries`、`oss-weekend-issues`、`pr-gate`）属 context-only 仓库自动化。
- **pre-commit**（`.husky/pre-commit`）：跑 `npm run check`，并对触及 `packages/ai/*`、`packages/web-ui/*`、`package.json`、`package-lock.json` 的暂存改动额外跑 browser smoke。这是 root → packages 的一条真实集成边。
- **脚本层**：`scripts/` 10 个直属文件（`build-binaries.sh`、`release.mjs`、`sync-versions.js`、`check-browser-smoke.mjs`、`profile-coding-agent-node.mjs`、`cost.ts` 等）被根 `package.json` 的 `check:browser-smoke`、`profile:*`、`version:*`、`release:*` 直接引用；`test.sh`/`pi-test.sh` 为根测试/运行入口。均为 context-only 集成面。
- **本地 pi 资源**：`.pi/{extensions,prompts}`（5 个 `.ts` 扩展 + 4 个 prompt）与 `pi-mono.code-workspace`（含外部 `../../moms`）为开发上下文，不属于冻结源。

## 子项接受的复用边界

- `ZS1-064`（`packages`）在本 work tree 蓝图为 `[x]`；其报告 `Docs/learn/stage1_pi_mono/packages/current_folder_learn.md` 存在于**集成后的 canonical checkout**（SHA256 `51605bddd9ad9356b6f57963023ceefd74f7ae0b086cb048a326673d98a5a90e`），已只读通读，作为 `ZS1-065` 的唯一直接输入。
- 该报告在**本隔离 work tree 中未物化**（本 work tree 停在 baseline `58a6f2a`，无 `packages/current_folder_learn.md`，亦无 `ZS1-062/063/064` 的 receipt）。本报告引用其当前内容但不重抄、不重算、不新增验收；其 worker 自测状态本身为 `[_]`，master `[x]` 由集成侧产生。
- `ZS1-064` 报告的递归叶子接受链（`061/062/063 → 057/058/059/060/… → 冻结 21 源文件`）与目标映射原样沿用，不在本层重复判定。

## 目标映射（仅引用）

本目录不新接受任何目标文件。root 层无直属冻结文件，唯一映射来自 `ZS1-064` → `ZS1-061/062/063` 的既有目标 owner 集合（`src/core.rs`/`src/runtime.rs`/`src/protocol.rs` 等，详见 `ZS1-064` 报告第 61 行），本报告不重复枚举、不新增义务。本 work tree 唯一已知目标差异仍是 `src/backend.rs` 实测 98999 B / `7b2e4b3cec6b59b23f2df436653faf990fd549dc7ac44fb2a559f323eeb88325` 对蓝图声明 99056 B / `0eae8058d36ef3507b2e1567f5d663843831a72ed73e506f33ec9ea18b034a43` 的 `target_baseline: current-worktree-with-user-changes` 用户改动，不在本项 owned path，**非阻塞状态差异**，交由 master/相关 owner。

## 本轮校验与替代记录

已执行（本 work tree / 只读源仓库）：

- 权威绑定：本 work tree 蓝图 SHA256 `7ff9cbe…` == claim `blueprint_digest`；`parse()` 复算 `requirement_digest = f69d1f67…`、`snapshot_sha256 = 7ff9cbe…`。
- `git -C /Users/mac/GitHub/pi-mono rev-parse HEAD` → `23282f60782f02b9e22b787e4b22af441454fa16`，与蓝图 header `source_revision` 逐字一致；`git status --short` 为空。
- 全蓝图源/目标指纹复算：58 个冻结行中，**41 个存在且逐一匹配** §3 表格 SHA256 与字节列；6 个不可回源重算的恰为声明集合 `ZS1-012/013/016/017/024/029`（非阻塞历史绑定）；10 个 reference/codex 项因参考根 `/Users/mac/GitHub/codex` 未挂载而本轮未核；1 个目标 `src/backend.rs` 为已知 `current-worktree-with-user-changes` 差异（见上）。现存冻结 source 文件哈希无一不符。
- root 直接清单（14 tracked 文件 + 6 tracked 目录）、各目录直接下层与递归计数、全部直属文件字节/SHA256 实测，均记录于上表；六个直接目录零符号链接。
- 组件级 G-DIR 闭包（Python 标准库 + validator 自带 `parse()`）：`ZS1-065` 的 `folder_path == "."`、`folder_artifact == Docs/learn/stage1_pi_mono/current_folder_learn.md`、`depends == [ZS1-064]`、`. ` 的直接子项集合 `{ZS1-064}` 与 `depends` 相等 → PASS。
- `python3 -m py_compile tools/validate_stage1_blueprint.py` → OK（exit 0）。
- `python3 tools/test_validate_stage1_blueprint.py` → `Ran 46 tests … OK`（exit 0），含目录闭包反例（`test_missing_directory_even_with_dependency_rewired_rejected`、`test_skip_immediate_child_directory_rejected`）与 selector/receipt/命令证据规则。

替代（声明的 gate 命令在本机不可完整执行）：

- `python3 tools/validate_stage1_blueprint.py`（默认草稿）与 `--item ZS1-065` 均 **exit 1**：`Docs/execution/active_requirement.json` 缺结尾换行（`incomplete JSON record (missing final newline)`），且其 `schema_version` 为旧 `active-requirement/1`、`blueprint_digest/snapshot_sha256 = ca9bade1…` 指向上一 bootstrap，与本 run 权威 `7ff9cbe…` 不一致。该文件不在本项 owned path，本节不修改它。
- 另有 scaffold 漂移：本 work tree 与 canonical 的 `folder_learn_index.tsv` 仍把 `ZS1-056/060/062/063/064` 记为 `[ ]`，而本 work tree 权威蓝图 `parse()` 记为 `[x]`；`ZS1-064` 的 report/receipt 也尚未物化于本 work tree。此为生成物滞后，非 `ZS1-065` 内容不一致；`check_scaffold` 级校验须待 master 修复 selector/index 后进行。
- G-STAGE `--item ZS1-065` 结构上还要求 `receipts/ZS1-065.master.json`，而 master receipt 只能由 master 在集成后生成；worker 无法也不应伪造。故以「组件级目录闭包 + 源指纹复算 + validator 单元套件」作为最接近等价验证，其**上限**是不做 receipt/selector 级联与集成行为验收。

限制：本轮未运行 Node/npm/tsx/tsgo/biome/vitest，未构建任何 `dist`，未执行 `npm run build`/`check`/`test`，未运行 `test.sh`/`pi-test.sh`/`scripts/*` 或任何产品入口，未访问 `.github`/`.husky`/`.pi`/`Docs`/`scripts` 的功能实现，未挂载 Codex 参考根重算 reference 指纹。以上不改变 `ZS1-064` 及其叶子的既有接受，也不新增产品行为验收。

## 状态与回滚

本候选只新增 `Docs/learn/stage1_pi_mono/current_folder_learn.md`，worker 自测状态为 `[_]`；`[x]` 只能由 master 在集成 frontier 复查源码与行为后写入。回滚仅撤去本候选报告，不递归修改 `ZS1-064` 子报告、其冻结叶子、上游源码、其它 owner、旧证据或产品代码。

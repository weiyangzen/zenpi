# ZS1-064 packages 独立目录整合报告 [_]

Worker（受限 claim `ZS1-064-20260917T030111-120`，layer `L2`，run `20260917T030111-120`，baseline `6914ad96c9665f249b9db4bcc54c3f7558761cf8`）。本候选只审阅 `packages` 的**目录整合关系**：直属文件与直接子目录的集成。正式直属子项恰为 `ZS1-061 packages/agent`、`ZS1-062 packages/ai`、`ZS1-063 packages/coding-agent`，三者在本 work tree 蓝图中均为 `[x]`；`packages` 无直属文件。`mom`、`pods`、`tui`、`web-ui` 四个直接目录及根 workspace 配置均为 context-only。父项 `ZS1-065`、任何目标 owner 与任何产品项**均不由本候选验收**。worker 自测只到 `[_]`，`[x]` 只能由 master 集成行为证据产生。

## 权威与源冻结绑定

- 当前权威为 `Docs/stage_1_v3_pi_mono_blueprint.md`（`blueprint_version 3.1.22`、`authoritative:true`）。本 work tree 该文件实测 SHA256 = `b58d07fc32e035cf8b6439eeaf92ab3885cdfcda6e6bbec2e5008533ec8062b4`，与本 claim 的 `blueprint_digest` 逐字一致。
- 本机 validator 组件 `parse()` 对本 work tree 权威全文复算 `requirement_digest = f69d1f67dc6a8238f891ca8583071c72397891a3adaa17169a6d9e8516b7bc76`，与 sibling `ZS1-062`/`ZS1-063` 记录一致。
- 第 31–37 行 `源冻结重签声明`：迁移前 `bbb61e34aaf231639fdaaad1adbd757947034eac` 在 origin 及本机均不可达；本 run 以 `source_revision 23282f60782f02b9e22b787e4b22af441454fa16` 重冻结；第 3 节源/目标行 SHA256（含可用字节数列）为**权威源指纹**；第 36 行明确 `ZS1-012/013/016/017/024/029` 源路径在当前 revision 已不存在，声明为历史报告一致性绑定，**缺源不构成 blocked**；历史报告内部旧 revision hash 一律被本声明取代。
- 本机 `source_repo`（`/Users/mac/GitHub/pi-mono`）HEAD 实测 = `23282f60782f02b9e22b787e4b22af441454fa16`，与蓝图 header `source_revision` 一致（clean，无未提交改动）。

## 真实直接清单与阅读边界

`packages` 为 **0 个直属文件、7 个直接目录**（`find packages -maxdepth 1 -type f` = 0）。下表全列；字节/SHA256 为本机只读 `source_repo` work tree 实测，递归计数含全部子目录文件（不构成验收）。下层只清点，不递归接受子树。

| 直接项 | 直接下层 | 递归文件/子目录 | `package.json` SHA256 | 范围与职责 |
| --- | --- | ---: | --- | --- |
| `agent/` | 7 项（5 文件+2 目录） | 17/3 | `3d0c93a46fd2eb5f8c689e19cea940f4eee39b8eb157841e0e394f9e24a07abd` | **正式 `ZS1-061`（`[x]`）** |
| `ai/` | 10 项（7 文件+3 目录） | 96/7 | `545599f9af2c43834d89cff4676ac85d66c105773c40963cc90ca9d1bef9b6c8` | **正式 `ZS1-062`（`[x]`）** |
| `coding-agent/` | 12 项（7 文件+5 目录） | 383/60 | `01fa5b00bb725b5e41269b6a1812c577328fbe1bbf493e373ff86bdff66c114d` | **正式 `ZS1-063`（`[x]`）** |
| `mom/` | 10 项（7 文件+3 目录） | 30/4 | `e15e4e7261e0b84c7c24ab452ca889c52e3d0780dc96bc720191c570ae57fcc2` | context-only；不在冻结子集 |
| `pods/` | 6 项（3 文件+3 目录） | 22/4 | `6c95f3f3c06e5fa834352e89715a25be72d236df72416090f132ebb22e6430ce` | context-only；不在冻结子集 |
| `tui/` | 7 项（5 文件+2 目录） | 56/3 | `0c1ccd2ae6f43d27eef9d5f57ab5b65a91cd3a2b3db4f422d74a477b7c608bd4` | context-only；不在冻结子集 |
| `web-ui/` | 8 项（5 文件+3 目录） | 87/15 | `0cff48a2a66725932b4e66cdfe0ec8c4f5b621a665837f305091e8379e78ee6f` | context-only；不在冻结子集 |

`packages` 自身没有 `package.json`、没有文件系统级标识；它只是根 workspace 的 glob 容器（`workspaces` 含 `packages/*` 与 4 个嵌套示例 workspace）。全 `packages` 递归 691 文件，其中仅 3 个 package、21 个源文件属于冻结 §3 子集，其余全部为 context-only。

## 冻结子集与直接子目录闭包

21 个冻结源文件分布在三个正式子包：`packages/agent`（`src/` 6 文件[含缺源2]、`test/` 1 文件）、`packages/ai`（`src/` 1 文件 + 缺源 3）、`packages/coding-agent`（`src/core/` 10 文件[含缺源 1]）。

| 目录项 | folder | Depends（=直接 in-scope 子项） | 状态 |
| --- | --- | --- | --- |
| `ZS1-061` | `packages/agent` | `ZS1-057,ZS1-058` | `[x]` |
| `ZS1-062` | `packages/ai` | `ZS1-059` | `[x]` |
| `ZS1-063` | `packages/coding-agent` | `ZS1-060` | `[x]` |
| `ZS1-064` | `packages` | `ZS1-061,ZS1-062,ZS1-063` | `[_]`（本报告） |
| `ZS1-065` | `.` | `ZS1-064` | `[ ]` |

- **G-DIR 闭包（组件级）**：validator `parse()` 计算 `packages` 的直接 in-scope 子项 = 直属冻结文件（无）∪ 直接冻结子目录 = `{ZS1-061, ZS1-062, ZS1-063}`，与行 `Depends` **逐字相等**；无缺项、无多余、无跳目录、无重复接受。
- **in-scope**：`ZS1-061/062/063` 及其递归冻结后代（`057,058 → 010/011/014/054 → 050/051`；`059 → 015/055 → 016/017/029`；`060 → 056 → 018…028/030 + 052/053`）。每一个叶子只被其唯一父目录项接受一次。
- **context-only**：`mom/`、`pods/`、`tui/`、`web-ui/` 四个直接目录、`packages` 无直属文件、根 workspace 配置（`package.json`/`tsconfig.base.json`）。它们被读取只为集成理解，不计入完成率、不形成目标义务。

## 跨包集成关系（本目录层新增的唯一分析）

- **根构建顺序**（`package.json` `build`）：`tui → ai → agent → coding-agent → mom → web-ui → pods`。依赖的编译产物先于消费者；`coding-agent` 的 `build:binary` 另行先建 `tui/ai/agent` 再编译 Bun 单文件。
- **冻结包依赖边**：`agent` → `ai`（`@mariozechner/pi-ai`）；`coding-agent` → `agent`、`ai`、`tui`（`@mariozechner/pi-tui`，另有 `jiti`、`diff`、`glob` 等三方依赖）。`ai` 无任何 pi 包依赖，是冻结子集内的叶。可见 `coding-agent` 依赖一个 **context-only** 包 `tui`：冻结子集内部无法自证该边，`tui` 的构建/契约仍属其自身 owner，本报告不据此新增验收。
- **context-only 包依赖边**：`mom` → `agent`、`ai`、`coding-agent`；`pods` → `agent`；`web-ui` → `ai`、`tui`。这些是源事实，说明冻结子集是更大 monorepo 的子图，而不是全仓。
- **版本锁步**：7 个包 `version` 全为 `0.62.0`，`type: module`，由根 `npm version -* -ws` + `scripts/sync-versions.js` 统一；冻结包与部分 context 包共享同一发布节奏，不构成功能验收。
- **编译基线**：所有包 `extends ../../tsconfig.base.json`（ES2022 / Node16 / strict / declaration + declarationMap + sourceMap）；`tsconfig.base.json` SHA256 = `478a425fe9f8f39dee52d1e3485e612e892113278e8f4430545600ad30a01bac`。源包各自 `tsconfig.build.json` 以 `rootDir=src → outDir=dist`；构建产物的可装载性本轮未验证。
- **两个入口语义**：`ai` 声明 12 个 `exports` 子路径与 `bin.pi-ai`；`coding-agent` 声明 `bin.pi` 与第二个导出 `"./hooks"`（其 `src/core/hooks/` 在当前 revision 不存在，`ZS1-063` 已记为 stale 源事实）。二者是包级一致性约束，不是本目录的验收对象。

## 子项接受的复用边界

- `ZS1-061`（`packages/agent`）已在蓝图 `[x]`，其报告 `Docs/learn/stage1_pi_mono/packages/agent/current_folder_learn.md` 在本 work tree 存在且非空（SHA256 `c458548fda48e1601376f795a8fbc72a23cf8114c2d388942695f56a843a9175`），完整正文已读，作为 `ZS1-064` 的直接输入。
- `ZS1-062`/`ZS1-063` 在蓝图 `[x]`，其报告存在于**集成后的 canonical checkout**（`packages/ai/current_folder_learn.md` SHA256 `93bbb02cfe00c98f1679a2e0b43614cdee8f993b5f79a4ed544286adf6759a64`；`packages/coding-agent/current_folder_learn.md` SHA256 `4f5eda1e73951ed63f17915e794484114f5374f2d0c65ffd090e954213fc6220`），已只读通读。二者在**本隔离 work tree 中未物化**（本 work tree 停在 baseline `6914ad96`），本报告引用其当前内容但不重抄、不重算、不新增验收。
- 三个子项各自的递归叶子接受链与目标映射原样沿用，不在本层重复判定。

## 目标映射（仅引用）

本目录不新接受任何目标文件。冻结子集的 Zenpi owner 映射沿用 `ZS1-061/062/063`：`src/core.rs`/`src/runtime.rs`/`src/protocol.rs`（G01/G02）、`src/context.rs`/`src/session.rs`（G03/G04）、`src/backend.rs`/`src/config.rs`/`src/providers/registry.rs`（G05）、`src/tools.rs`（G06/G07）、`src/skills.rs`/`src/slash.rs`（G08）、`src/extensions.rs`（G09）、`src/headless.rs`/`src/tui.rs`（宿主入口）。

本 work tree 目标 owner 实测：`src/core.rs`、`src/runtime.rs`、`src/context.rs`、`src/session.rs`、`src/config.rs`、`src/tools.rs`、`src/skills.rs`、`src/slash.rs`、`src/extensions.rs`、`src/headless.rs`、`src/lib.rs` 的字节/SHA256 均与蓝图第 3 节目标表逐字一致；**唯一差异**是 `src/backend.rs` 实测 98999 B / `7b2e4b3cec6b59b23f2df436653faf990fd549dc7ac44fb2a559f323eeb88325`，蓝图表声明 99056 B / `0eae8058d36ef3507b2e1567f5d663843831a72ed73e506f33ec9ea18b034a43`。这是目标 worktree 在冻结表之后的用户改动（`target_baseline: current-worktree-with-user-changes`），不在本项 owned path，记为**非阻塞状态差异**交由 master/相关 owner 处理，不据此判 `blocked`。

## 本轮校验与替代记录

已执行（本 work tree / 只读源仓库）：

- 权威绑定：本 work tree 蓝图 SHA256 `b58d07fc…` == claim `blueprint_digest`；`parse()` 复算 `requirement_digest = f69d1f67…`、`snapshot_sha256 = b58d07fc…`。
- `git -C /Users/mac/GitHub/pi-mono rev-parse HEAD` → `23282f60782f02b9e22b787e4b22af441454fa16`，与蓝图 header `source_revision` 逐字一致。
- 全蓝图源指纹复算：21 个源文件中 **15 个存在且逐一匹配** §3 表格 SHA256；缺失的恰为声明集合 `ZS1-012/013/016/017/024/029`（六项，非阻塞历史绑定）。无一个存在的冻结源文件哈希不符。
- `packages` 直接清单（0 文件 + 7 目录）、每目录直接下层与递归计数、7 个 `package.json` / `tsconfig.base.json` 的字节与 SHA256 实测，均记录于上表。
- 组件级 G-DIR 闭包（Python 标准库 + validator 自带 `parse()`）：`ZS1-064` 的 `folder_path == packages`、`folder_artifact == Docs/learn/stage1_pi_mono/packages/current_folder_learn.md`、`depends == [ZS1-061, ZS1-062, ZS1-063]`、直接子项集合与 `depends` 相等、三个子项状态 `[x]` → PASS。
- `python3 -m py_compile tools/validate_stage1_blueprint.py` → OK（exit 0）。
- `python3 tools/test_validate_stage1_blueprint.py` → `Ran 46 tests ... OK`（exit 0），含目录闭包反例（`test_missing_directory_even_with_dependency_rewired_rejected`、`test_skip_immediate_child_directory_rejected`）与 selector/receipt/命令证据规则。

替代（声明的 gate 命令在本机不可完整执行）：

- `python3 tools/validate_stage1_blueprint.py`（默认草稿）与 `--item ZS1-064` 均 **exit 1**：`Docs/execution/active_requirement.json` 缺结尾换行（`incomplete JSON record (missing final newline)`），且其 `schema_version` 为 `active-requirement/1`、`blueprint_digest/snapshot_sha256 = ca9bade1…` 指向上一 bootstrap，与本 run 权威 `b58d07fc…` 不一致。该文件不在本项 owned path，本节不修改它。
- 另有 scaffold 漂移：本 work tree `folder_learn_index.tsv` 仍把 `ZS1-056/060/062/063` 记为 `[ ]`，而权威蓝图 `parse()` 记为 `[x]`。此为生成物滞后，非 `ZS1-064` 内容不一致；`check_scaffold` 级校验须待 master 修复 selector/index 后进行。
- G-STAGE `--item ZS1-064` 结构上还要求 `receipts/ZS1-064.master.json`，而 master receipt 只能由 master 在集成后生成；worker 无法也不应伪造。故以「组件级目录闭包 + 源指纹复算 + validator 单元套件」作为最接近等价验证，其**上限**是不做 receipt/selector 级联与集成行为验收。

限制：本轮未运行 Node/Bun/npm/tsx/vitest/tsgo，未构建任何 `dist`、未执行 `generate-models.ts`（含联网）、未运行任何包测试、CLI 或产品入口，未访问 `mom`/`pods`/`tui`/`web-ui` 的功能实现，未重读缺源的 `ZS1-016/017/029`。以上不改变 `ZS1-061/062/063` 及其叶子的既有接受，也不新增产品行为验收。

## 状态与回滚

本候选只新增 `Docs/learn/stage1_pi_mono/packages/current_folder_learn.md`，worker 自测状态为 `[_]`；`[x]` 只能由 master 在集成 frontier 复查源码与行为后写入。回滚仅撤去本候选报告，不递归修改 `ZS1-061/062/063` 子报告、其冻结叶子、上游源码、其它 owner、旧证据或产品代码。

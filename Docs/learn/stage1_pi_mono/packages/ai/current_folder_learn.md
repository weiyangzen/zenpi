# ZS1-062 packages/ai 独立目录整合报告 [_]

Worker（受限 claim `ZS1-062-20260917T024947-106`，layer `L2`）。本候选只审阅 `packages/ai` 的**目录整合关系**：直属文件与直接子目录的集成。正式直属子项只有 `ZS1-059` `packages/ai/src`（`[x]`）；`packages/ai` 的 7 个直接文件与 `scripts`、`test` 两个直接目录均为 context-only。父项 `ZS1-064` `packages`、产品项（`ZS1-107/108/109/110` 等）以及本 package 的构建、发布或运行行为**均不由本候选验收**。worker 自测只到 `[_]`，`[x]` 只能由 master 集成行为证据产生。

本报告在本 claim 的 work tree 内**从当前源仓库重新推导**；同 subject 的先前候选（`ZS1-062-20260917T024046-110`）位于另一个独立 work tree，其字节未复制进本报告，所有清单与指纹均在本 work tree / 只读源仓库重新实测（见“本轮校验与替代记录”）。

## 权威与源冻结绑定

- 当前权威为 `Docs/stage_1_v3_pi_mono_blueprint.md`（`blueprint_version 3.1.22`、`authoritative:true`）。本 work tree 该文件 SHA256 = `cd22c9dd1bac28fe17e89b8f9e36dc6df806fbf7c4fde86a5bdc1fe8a1a0b7d1`，与本 claim 的 `blueprint_digest` / `snapshot_sha256` 逐字一致（本机 validator `parse()` 复算一致）。
- 本机 validator 组件对本 work tree 权威全文复算 `requirement_digest = f69d1f67dc6a8238f891ca8583071c72397891a3adaa17169a6d9e8516b7bc76`（与 sibling `ZS1-059` claim 记录一致）。
- 第 31–37 行 `源冻结重签声明`：迁移前 `bbb61e34aaf231639fdaaad1adbd757947034eac` 在 origin 与本机均不可达；本 run 以 `source_revision 23282f60782f02b9e22b787e4b22af441454fa16` 重冻结；第 3 节源/目标行 SHA256（含可用字节数列）为**权威源指纹**；第 36 行明确 `ZS1-012/013/016/017/024/029` 源路径在当前 revision 已不存在，声明为历史报告一致性绑定，**缺源不构成 blocked**；历史报告内部旧 revision hash 一律被本声明取代。
- 本机 `source_repo`（`/Users/mac/GitHub/pi-mono`）HEAD 实测 = `23282f60782f02b9e22b787e4b22af441454fa16`，与蓝图 header `source_revision` 一致。
- 对 `ZS1-062` 的直接后果：正式子项 `ZS1-059` 的源目录 `packages/ai/src` 存在，其正式叶子 `ZS1-015`（`src/types.ts`）live SHA256 与蓝图第 111 行表格指纹逐字一致；孙项 `ZS1-055` 的源目录 `packages/ai/src/api` 在当前 revision **不存在**（`test -d packages/ai/src/api` → ABSENT）。按第 36 行声明，`api` 的目录理解复用其历史目录报告 `Docs/learn/stage1_pi_mono/packages/ai/src/api/current_folder_learn.md`，本节**不重读、不重算、不据此新增验收**。当前存在的 `src/providers/anthropic.ts`、`google.ts`、`openai-completions.ts` 与历史 `api/` 下同名文件**路径与 revision 均不同**，不构成 `ZS1-016/017/029` 的重命名，不得被读作那三项的当前源。

## 真实直接清单与阅读边界

当前 `packages/ai` 为 **7 个直接文件、3 个直接目录**。下表全列；字节/行/SHA256 为本机 `source_repo` work tree 实测。下层清单只清点，不递归接受子树。

| 直接项 | 字节/行 | 当前 SHA256 | 范围与职责 |
| --- | ---: | --- | --- |
| `CHANGELOG.md` | 59336/909 | `c0966094f47f15d8b4a1bf1d87a5030a626e44f3017f64e313294202b8efd679` | context-only；历史版本与接口迁移说明，非行为证明 |
| `README.md` | 46383/1229 | `bcdd6a051032546e5b86f3f4f5e6438283b43c544ed321de940db5665f4c85fd` | context-only；消费示例、事件/停止原因/错误与 abort 说明 |
| `bedrock-provider.d.ts` | 44/1 | `8971c230fb2d216f2f94b590906a8a4e948bd72edae6ce131377daff5b4c4c3b` | context-only；`bedrock-provider` 顶层 shim 的类型再导出 |
| `bedrock-provider.js` | 44/1 | `8971c230fb2d216f2f94b590906a8a4e948bd72edae6ce131377daff5b4c4c3b` | context-only；`export * from "./dist/bedrock-provider.js"`（与 `.d.ts` 同字节） |
| `package.json` | 3019/114 | `545599f9af2c43834d89cff4676ac85d66c105773c40963cc90ca9d1bef9b6c8` | context-only；包出口、bin、scripts、依赖与 engines 声明 |
| `tsconfig.build.json` | 208/8 | `79bc221343ee66cf1e9b7cfd56d7e5d61957010e756cd773b8d07422b26ba79e` | context-only；src→dist 编译范围 |
| `vitest.config.ts` | 190/8 | `c3dffe9e47403d29e3289e809b3e7edaa77e610c04b068d0d7db0174730d7ec0` | context-only；测试发现/超时配置，本项未执行 |
| `scripts/` | 目录（2 文件，0 子目录） | — | context-only；构建期模型目录生成器与测试图片生成器 |
| `src/` | 目录（10 文件、2 子目录） | — | **正式 `ZS1-059`（`[x]`）**，本 package 唯一正式直接子项 |
| `test/` | 目录（45 项：41 `*.test.ts`、3 helper、1 `data/`） | — | context-only；无 `packages/ai` 级正式测试项 |

`scripts/` 实测下层：`generate-models.ts`（47431/1593，`f0c55835718781af7e1a0a6d53101010b229e33a59532b2b9dc2e79a5ba10bfd`）、`generate-test-image.ts`（943/33，`31024fadcebba4e80038b8bc64c9483ddde4b5a3c2f42000b9a95d4d2c5e3359`）。

`src/` 实测下层 10 文件：`api-registry.ts`（2563/98，`71fe243b2179761ced7833415f7a772202447a459806a087121fa8ff70c37339`）、`bedrock-provider.ts`（165/6，`23f8d54518b415836dfdb4862486aed7e6dd7d8a2375c7ed03819b4ecd95a627`）、`cli.ts`（3864/133，`5da99f5f18404e54561a2fd955751a7767645af98d4bd10f747e5d8cebb6cee5`）、`env-api-keys.ts`（5023/133，`33cfcd8dcfee72a70e18dc27947e90b1fc750978e2762a5b16cb2b366987ce6b`）、`index.ts`（1477/34，`bd5dd6eea1505acb9b00cf78f92fc7299505160f7ca8f951c4d8c685111fa66b`）、`models.generated.ts`（352191/13899，`cdc8f279ca5286daab8aa32935a4f275a6564155abb63840ae3868e87bace1d7`；生成物，只读静态内容，未逐行复核、未执行生成器）、`models.ts`（2888/77，`97cdb6266f7c58d84e560768b87215397f7dbd622a267de5e334c0a8faa2d345`）、`oauth.ts`（40/1，`ed04bcd233ee6a4b16d19041012ea92fbed53ef4aa8c30851ac4c5a4cc9c97fb`）、`stream.ts`（1486/59，`09b24abc904a6c3c5cfe16d30a2e1c09239a5e3c98cb70d9572f50202f4d5415`）、`types.ts`（12043/337，`ddc294dc3ec0f84165e4f3b389dac59fd736cde8c496c4d84dee86ef017551f4`；正式 `ZS1-015`，== 蓝图第 111 行表格指纹）。`src/` 的 2 个直接子目录 `providers/`（16 文件、0 子目录）与 `utils/`（8 文件、1 子目录 `oauth/` 内 9 文件）均为 context-only，不在本项冻结子集内。

`test/` 实测下层：41 个 `*.test.ts`、3 个 helper（`azure-utils.ts`、`bedrock-utils.ts`、`oauth.ts`）与 `data/`（1 文件 `red-circle.png`，2565 B）。`test/` 无正式 062 直接子项；本项只清点目录构成，未运行任何用例。

## 包入口、导出、构建与依赖边界

- `package.json`：`@mariozechner/pi-ai` 0.62.0，`type: module`，`main ./dist/index.js`、`types ./dist/index.d.ts`，`engines.node >=20.0.0`。
- `exports` 共 12 项：根 `.`、`./anthropic`、`./azure-openai-responses`、`./google`、`./google-gemini-cli`、`./google-vertex`、`./mistral`、`./openai-codex-responses`、`./openai-completions`、`./openai-responses`、`./oauth`、`./bedrock-provider`。除根与 `oauth`/`bedrock-provider` 外，每个子路径声明指向 `./dist/providers/<name>.js`。这些是**声明**，本轮未构建、未验证 `dist` 实际生成或 npm 包可安装。
- `bin.pi-ai` → `./dist/cli.js`；`files` = `["dist","README.md"]`。
- `scripts`：`clean`（`shx rm -rf dist`）、`generate-models`（`npx tsx scripts/generate-models.ts`）、`build`（`npm run generate-models && tsgo -p tsconfig.build.json`）、`dev`/`dev:tsc`（`tsgo ... --watch`）、`test`（`vitest --run`）、`prepublishOnly`（`npm run clean && npm run build`）。`build` 先联网生成模型目录再编译；本轮**未执行**任何脚本。
- `tsconfig.build.json` extends `../../tsconfig.base.json`，`rootDir=./src`、`outDir=./dist`、`include ["src/**/*.ts"]`、排除 `node_modules`/`dist`/`*.d.ts`/`src/**/*.d.ts`。编译输入是 `src` 的 10 个文件（含生成的 `models.generated.ts`），输出语义依赖根 workspace 的 base 配置。
- `vitest.config.ts`：`globals:true`、`environment:'node'`、`testTimeout:30000`，无 `include` 覆盖 → 使用 vitest 默认发现范围。这是发现/超时边界，不是已运行结果。
- `bedrock-provider.js`/`.d.ts`：单行 `export * from "./dist/bedrock-provider.js"`，为 Bedrock 提供顶层消费 shim（对应 `src/bedrock-provider.ts`）。
- 运行依赖：`@anthropic-ai/sdk ^0.73.0`、`@aws-sdk/client-bedrock-runtime ^3.983.0`、`@google/genai ^1.40.0`、`@mistralai/mistralai 1.14.1`、`@sinclair/typebox ^0.34.41`、`ajv ^8.17.1`、`ajv-formats ^3.0.1`、`chalk ^5.6.2`、`openai 6.26.0`、`partial-json ^0.1.7`、`proxy-agent ^6.5.0`、`undici ^7.19.1`、`zod-to-json-schema ^3.24.6`；dev：`@types/node ^24.3.0`、`canvas ^3.2.0`、`vitest ^3.2.4`。仅核对声明，未审计安装树或 lock。

## 调用关系、数据所有权与跨文件不变量（直属 + src 集成）

- **入口 barrel**：`src/index.ts` 导出 `api-registry`、`env-api-keys`、`models`、`stream`、`types`、`providers/register-builtins`、`utils/event-stream`、`utils/json-parse`、`utils/overflow`、`utils/typebox-helpers`、`utils/validation`，并以 type-only 导出各 provider options 与 `utils/oauth/types`。它**不**直接导出生成目录（`models.generated`）或 OAuth flow 实现；`Type`/`Static`/`TSchema` 从 `@sinclair/typebox` re-export。
- **注册副作用与分派**：`src/stream.ts` 首行 `import "./providers/register-builtins.js"`，模块加载即执行注册副作用。`stream()`/`streamSimple()` 经 `resolveApiProvider(model.api)` 取 provider，缺项时**同步** `throw new Error("No API provider registered for api: ...")`。`Model.api` 是唯一 wire 分派键；`Model.provider` 只用于模型身份。
- **注册所有者**：`src/api-registry.ts` 以 `Map<api,{provider,sourceId}>` 持有注册表；`registerApiProvider(provider, sourceId?)` 用 `wrapStream`/`wrapStreamSimple` 在**调用时**校验 `model.api === api`，不匹配即 `throw new Error("Mismatched api: ...")`；`unregisterApiProviders(sourceId)` 按来源批量删除；`clearApiProviders()` 清表。注册表是**进程内单例，不持久化**。
- **内置 provider**：`src/providers/register-builtins.ts` 模块尾部直接调用 `registerBuiltInApiProviders()`，按固定顺序注册 10 个 api：`anthropic-messages`、`openai-completions`、`mistral-conversations`、`openai-responses`、`azure-openai-responses`、`openai-codex-responses`、`google-generative-ai`、`google-gemini-cli`、`google-vertex`、`bedrock-converse-stream`。各 provider 以模块级缓存 Promise **lazy import**；`createLazyStream`/`createLazySimpleStream` 的 `.catch` 把导入失败转成 `stopReason:"error"` 的 `AssistantMessage` + `error` 事件，而非同步抛出。`setBedrockProviderModule` 允许外部注入覆盖 Bedrock；`resetApiProviders()` 先 `clearApiProviders()` 再注册。
- **模型目录数据**：`src/models.ts` 在模块加载时从 `models.generated.ts` 的 `MODELS` 构建 `Map<provider, Map<id, Model>>`。`getModel()` 可能返回 `undefined` 却被 `as Model<...>` 强转；`getProviders()`/`getModels()` 返回目录视图；`calculateCost()` **原地**写 `usage.cost` 并按 1e6 缩放；`supportsXhigh()` 按 id 子串（`gpt-5.2`/`gpt-5.3`/`gpt-5.4`、`opus-4-6`/`opus-4.6`）匹配；`modelsAreEqual()` 只比较 `id+provider`。
- **认证/环境所有权**：`src/env-api-keys.ts` 的 `getEnvApiKey(provider)` 只读环境：`anthropic` 优先 `ANTHROPIC_OAUTH_TOKEN` 于 `ANTHROPIC_API_KEY`；`github-copilot` 取 `COPILOT_GITHUB_TOKEN`/`GH_TOKEN`/`GITHUB_TOKEN`；`google-vertex` 在 `GOOGLE_CLOUD_API_KEY` 缺失且 ADC 文件存在 + project + location 齐备时返回 `"<authenticated>"`；`amazon-bedrock` 多来源（profile/IAM/bearer/ECS/IRSA）返回 `"<authenticated>"`。Node/Bun 分支经异步动态 import 加载 fs/os/path；它只读环境，不写凭证。
- **CLI 入口**：`src/cli.ts` 是独立 `main()`（导入即执行），`bin` 指向 `dist/cli.js`。`AUTH_FILE = "auth.json"` **相对 cwd**；`loadAuth` 对缺失/坏 JSON 宽容为 `{}`；`saveAuth` 整文件同步覆写；登录经 `readline`；未知命令 `process.exit`。本轮未运行 CLI、未访问真实 `auth.json`。
- **生成脚本**：`scripts/generate-models.ts`（1593 行）抓取远端模型目录源并渲染 `src/models.generated.ts`；`scripts/generate-test-image.ts` 用 `canvas` 生成 `test/data/red-circle.png`。两者均为构建期工具，**本轮未执行**、未触网。
- **`oauth.ts`**：单行 `export * from "./utils/oauth/index.js"`，只是 re-export 入口。
- **跨文件不变量**：① `exports` 的每个 provider 子路径须与 `src/providers/*.ts` 及编译产物 `dist/providers/*.js` 对应；② `register-builtins` 的 10 个注册 api 字符串须与 `types.ts` 的 `KnownApi` 联合类型及各 provider 模块导出的 `stream*`/`streamSimple*` 名称对应；③ `Model.api` 必须命中注册表，否则同步 throw；④ `api-registry` 的 `wrap*` 在 `model.api !== api` 时同步 throw，与 `resolveApiProvider` 缺项 throw 是两条独立错误边界；⑤ 生成目录是构建期静态数据，运行期不刷新。

## 错误、取消、持久化与目标映射

- **错误/终态**：流式结果的完成/失败由 `AssistantMessageEventStream` 的 `done`/`error` 承载；`stopReason` 域为 `stop`/`length`/`toolUse`/`error`/`aborted`。`stream.ts` 的同步 throw 只发生在 `resolveApiProvider` 缺项；`api-registry` 的 `wrap*` 在 api 不匹配时同步 throw；lazy 加载失败则转为 error 事件。这三类边界必须分开解释。`complete`/`completeSimple` 只是 `await stream(...)` 的 `result()`，终态 error 消息不等同 Promise 拒绝。
- **取消**：README 记载通过 `options.signal`（`AbortSignal`）取消，`aborted` 消息可追加进后续 context 继续（"Continuing After Abort"）。这是源侧合作式取消契约；本项只读，未运行。
- **持久化**：`src` 无磁盘会话/目录持久化；模型目录与注册表均为进程内/构建期。唯一落盘在 `cli.ts` 的 `auth.json`（相对 cwd、同步整文件写、无原子替换），是独立 CLI 行为，不参与库运行时。
- **目标映射（沿用已接受 `ZS1-015`/`ZS1-055`，限定当前 revision）**：`types.ts` 的 provider/model 能力、reasoning、输入类型、`contextWindow` 映射到 `src/backend.rs:121` 与 `src/config.rs`（G05）；wire 分派与终止块校验映射到 `src/backend.rs` 与 `src/providers/registry.rs`；回合/工具/会话/取消组合仍由 `src/core.rs`、`src/headless.rs` 所有。因 `ZS1-016/017/029` 缺源，当前 revision 无新的 wire 源可重读，历史 `ZS1-055` 对这些路径的映射仍是权威边界。`scripts/`、生成目录、CLI、`test/` 为 context-only，**不构成目标交付义务**。
- 本 work tree 目标 owner 文件实测（与蓝图“zenpi 目标 owner 文件”表逐字一致）：`src/core.rs` 279620 B `0d4d0d1aef706528fe1a91ec9846ab4ea365c61e08c67d23a6a794a26c49916e`、`src/backend.rs` 99056 B `0eae8058d36ef3507b2e1567f5d663843831a72ed73e506f33ec9ea18b034a43`、`src/config.rs` 77782 B `f5691bd3818dcaf1481a88b63cc4f3874bc6b007234059e26124b74046325d12`、`src/headless.rs` 371210 B `c2aefb53e5bc342af94eab463445373a83fbca172aa235246bb0cd9aa64f65bb`。另有 `src/providers/registry.rs` 15905 B `cf229a62681933e7c1b4058c4d2fada41dff4fcd05c4cd300e18accfe245b944`（无独立 ZS1 owner 表项，仅映射引用）。本项不做产品行为验收。

## G-DIR 闭包（in-scope / context-only）

- **in-scope（正式直接子项，唯一）**：`ZS1-059` → `packages/ai/src`，状态 `[x]`，报告 `Docs/learn/stage1_pi_mono/packages/ai/src/current_folder_learn.md` 存在且非空。
- **in-scope（孙项，历史一致性绑定）**：`ZS1-055` → `packages/ai/src/api`，状态 `[x]`；源路径在当前 revision ABSENT（第 36 行声明），只复用其历史目录报告，本节不重读/不重算/不新增验收。
- **context-only**：7 个直接文件（`CHANGELOG.md`、`README.md`、`bedrock-provider.d.ts`、`bedrock-provider.js`、`package.json`、`tsconfig.build.json`、`vitest.config.ts`）+ 直接目录 `scripts/`、`test/` + `src` 下非正式子目录 `providers/`、`utils/`。它们不是本项冻结子集中的文件/目录项，读取只为集成理解。
- **闭包链**：`ZS1-062 → ZS1-059 → {ZS1-015, ZS1-055} → ZS1-055 → {ZS1-016, ZS1-017, ZS1-029}`。本机 validator `parse()` 复核：`ZS1-062` 为 `L2`、`loc=0`、`owned_paths` 恰为唯一报告、`depends` 恰为直接子项 `{ZS1-059}`、`validators` 为 `G-DIR；G-STAGE --item ZS1-062`、folder 映射为 `('source','packages/ai',报告)`。检查结果：**无跳目录、无重复接受、无遗漏**。本报告不代父项 `ZS1-064` 验收，也不宣称本目录未纳入文件已验收。

## 本轮校验与替代记录

已执行（本 work tree / 只读源仓库）：

- 权威绑定：本 work tree 蓝图 SHA256 `cd22c9dd…` == claim `blueprint_digest`；validator `parse()` 复算 `requirement_digest = f69d1f67…`、`snapshot_digest = cd22c9dd…`。
- `git -C /Users/mac/GitHub/pi-mono rev-parse HEAD` → `23282f60782f02b9e22b787e4b22af441454fa16`，与蓝图 header `source_revision` 逐字一致（exit 0）。
- `shasum -a 256 packages/ai/src/types.ts` → `ddc294dc3ec0f84165e4f3b389dac59fd736cde8c496c4d84dee86ef017551f4`，与蓝图第 111 行表格指纹逐字一致（exit 0）。
- 直接项 7 文件 + `src` 10 文件 + `scripts` 2 文件的字节/行/SHA256 清点；`test/` 45 项（41 test + 3 helper + `data/`）与 `providers`（16）/`utils`（8+1）下层清点。
- `test -d packages/ai/src/api` → ABSENT（exit 1，符合第 36 行声明）。
- `python3 -m py_compile tools/validate_stage1_blueprint.py` → OK（exit 0）。
- `python3 tools/test_validate_stage1_blueprint.py` → `Ran 46 tests ... OK`（exit 0，含 `test_missing_directory_even_with_dependency_rewired_rejected`、`test_skip_immediate_child_directory_rejected` 等目录闭包反例）。
- 组件级 G-DIR 结构闭包检查（Python 标准库 + validator 自带 `parse()`，脚本置于 `/tmp/opencode`）：`ZS1-062` 的 `folder_path == packages/ai`、`folder_artifact == 本报告`、`depends == [ZS1-059]`、`ZS1-059` 状态 `[x]` 且其报告存在、本报告存在且非空 → PASS。
- 目标 owner 4 文件的 SHA256/字节实测与蓝图目标表逐字一致（见上节）。

替代（声明的 gate 命令在本机不可完整执行）：

- `python3 tools/validate_stage1_blueprint.py --json`（默认草稿）与 `--item ZS1-062` 均 **exit 1**，原因是 `Docs/execution/active_requirement.json` **缺结尾换行**（`incomplete JSON record (missing final newline)`），且其 `schema_version` 为 `active-requirement/1`（validator 需要 `stage1-selector/v1`）、`blueprint_digest/snapshot_sha256 = ca9bade1…` 属上一 bootstrap，与本 run 权威 `cd22c9dd…` 不一致。该文件不在本项 owned path，本节不修改它。
- G-STAGE `--item ZS1-062` 在结构上还要求 `receipts/ZS1-062.master.json`，而 master receipt 只能由 master 在集成后生成；worker 无法也不应伪造。故以「组件级目录闭包结构检查 + 源指纹核对 + validator 单元套件」作为最接近等价验证，其**上限**是不做 receipt/selector 级联校验与集成行为验收。

限制：本轮未运行 Node/Bun/npm/tsx/vitest，未构建 `dist`，未执行 `scripts/generate-models.ts`（含联网）或 `generate-test-image.ts`，未运行 CLI/OAuth，未访问真实凭证或 `auth.json`，未重读缺源的 `ZS1-016/017/029`，未验收任何 context-only 文件；`models.generated.ts`（352191 B）仅按生成静态目录登记。以上不改变 `ZS1-015`/`ZS1-055`/`ZS1-059` 既有接受，也不新增产品行为验收。

## 状态与回滚

本候选只新增 `Docs/learn/stage1_pi_mono/packages/ai/current_folder_learn.md`，worker 自测状态为 `[_]`；`[x]` 只能由 master 在集成 frontier 复查源码与行为后写入。回滚仅撤去本候选报告（或按 claim 撤整个报告），不递归修改 `ZS1-059`/`ZS1-055`/`ZS1-015`、上游源码、其它 owner、旧证据或产品代码。

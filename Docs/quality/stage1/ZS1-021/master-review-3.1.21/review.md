# ZS1-021 主控独立复核（3.1.21）

结论：接受且仅接受 `packages/coding-agent/src/core/resource-loader.ts` 的完整源行为理解与当前目标映射。目标功能、目标文件及父目录各自保留独立验收；本结论不关闭 ZS1-056、ZS1-114 或扩展生命周期事项。原候选与失败历史均保留，以下限定优先于旧报告中较宽的表述。

## 独立阅读与身份

主控在本连续任务中完整阅读 40180 字节、1097 行源文件，SHA256 `1877e9535820cb8b45e5a84598ac8ae581058bb0fbee6f0c64ec672f8ab986dd`。六段实际阅读为 1–190、191–390、391–590、591–790、791–990、991–1097，对应半开字节区间 [0,7471)、[7471,15291)、[15291,23941)、[23941,31274)、[31274,37251)、[37251,40180)。已完整阅读 47827 字节、237 行候选及其旧报告前缀，完整阅读历史 probe.ts 17360 字节、71 行；本次补读三个原始运行记录与完整日志、1198 项离线检查器源码和输入/阅读清单。

目标 context：完整阅读 resource_loader.rs 1–280；独立阅读 skills.rs 142–262、281–317，extensions.rs 161–311，prompt_templates.rs 78–195，core.rs 2108–2211、2212–2345，slash_actions.rs 46–146。worker 的 T9 6244–6315 为失效定位的偶然阅读，主控未读、不计入主控阅读范围。旧 C 恢复 helper 在当前 6347–6418 的定位仅为候选的逐字身份复用，主控不把它记为新阅读。完整复制的其它目标/测试/依赖文件仅用于身份核对；不宣称本次全量阅读或执行它们。

包 manifest SHA256 `624213df1e2b5ad2d9f943440d55c11951c04e38b6d39f714c8cc687479d4ea4`，355 个 payload。主控新运行一次包内纯 Python 离线检查器，cwd=/tmp，输出置于包外，1198 项全部通过；这些是路径、哈希、范围、证据关联检查，绝不是 1198 个行为测试。当前主库 9 个捕获目标与包中身份全部一致，当前上游源文件逐字一致。旧 authority 仅作为当时事实保留；本次验收使用当前 selector、蓝图与正式 001 receipt。

## 逐块理解及迁移决定

1. 构造保存路径、数组、factory 与 override 等引用并初始化空结果，不自动 reload；默认 SettingsManager.create 的副作用不归零。多数 getter 交出内部引用，source getter 才新建记录，不能据此认为源 catalog 不可变。
2. resolvePromptInput 把存在路径作为 UTF-8 文件尝试读取并剥 BOM；读取失败警告后保留原文字。路径来源记录与读取成功/override 后内容不是同一个保证。自动 SYSTEM/APPEND 使用受信任项目 `.pi` 或 global，不遍历所有祖先；显式空 system/append 抑制自动发现。独立 context 搜索却按 global 后根到 cwd 的顺序，按 AGENTS.override/AGENTS/CLAUDE 的候选优先级读取 regular file，读失败尝试下一名字，不在普通 Git 根停止，也不使用同一个 trust 过滤。
3. linked-worktree 的影子过滤依赖真实主仓和 common Git 目录布局及相同 basename，不是任意重复内容去重，也不能推广到所有 worktree/submodule。文件可能先读取、后被过滤。
4. reload 会先清 metadata/扩展贡献，并按顺序改字段；extendResources 先更新三个来源 map 再重新加载 skills/prompts/themes。late override 或 IO 抛错可留下部分新状态。loaded 只在成功结尾置位，清 cache 的条件不构成事务；无统一 AbortSignal、并发 generation 或 journal 恢复。这些是源行为，不要求 Zenpi 引入同样的失败语义。
5. metadata 保存第一条路径记录，包含 disabled；no* 只关闭特定默认集合，CLI/additional、inline/override 仍有入口。自动/package 技能目录映射存在的 SKILL.md，但此 exists 判断不独自证明 regular file。路径 canonical 去重保留首个 resolved 拼写；来源查找先额外 map 的首个祖先，然后 metadata exact/首个祖先，不是最长前缀。
6. bootstrap 先设未信任并重新加载设置，然后预载允许的 global/CLI/inline，之后才等待 trust callback。失败不保证恢复 trust。成功预载按 resolvedPath 复用、失败集合阻止第二次尝试；inline 不再次执行。最终过滤不撤销已执行副作用。冲突检查实际覆盖 tools/flags，不覆盖注释提及的 commands，追加错误也不删除 module。
7. prompts/themes 先按 name 去重、再 override；override 可重引同名。theme 对象的 sourceInfo 原位赋值，目录仅扫描直接 JSON，regular symlink 可接受，没有统一目录项/读取预算。诊断可因既有 warning 而不再追加 missing error。skills 的 sourceInfo 重建与 themes 的原位修改须区分。

## Zenpi 原子性的准确范围

ResourceLoader 在独占可变访问下完成路径正规化、技能/模板/具名文本候选和最后取消检查，才替换路径与 Arc snapshot，generation 溢出报错。具名文本有 64 份/1 MiB 总量及名称约束。Arc 保证该 catalog 的共享身份，不能冻结尚未读取的磁盘技能正文；正文仍需新读与 source hash 检查。技能 user→project→explicit 覆盖、同 scope TOML 优先；模板同 scope 同名报错、跨 scope 覆盖，与 Pi 首名保留加 warning 的规则不同。

主控确认 core.configure_resources 首先拒绝 Closed；只有在需要准备已配置扩展时，prepare_extensions 的 Idle 限制才被调用，不能把所有资源重载都描述成无条件 Idle-only。资源与扩展准备/取消检查完成后先 append provenance，再发布资源与扩展。publish_extensions 会 close 旧 runtime、替换 registry/runtime、start 新 runtime；hook 错误变 AgentEvent::Error，不回滚已写 journal 或已发布资源。因此“资源候选失败不发布”成立，“所有扩展副作用与持久化是一笔可回滚事务”不成立。

slash_actions 的选择文件参数有 relative、无 ParentDir/absolute、4096 字节限制并透传取消；本次阅读没有完整追踪 read_skill_text 的路径链验证，故旧候选“workspace 内”仅确认为这段的词法限制和相对路径锚定，不用来证明所有祖先 symlink 都无法越界。该限定不代表发现实际逃逸反例。完整 host 调度、真实 PTY、跨平台行为继续由各自事项验收。

## 历史行为证据与失败保留

已逐案对照实际 probe 和日志：源 controller、文件系统、skills/prompts parser、路径/sourceInfo/Git finder 参与；package-manager、settings-manager、extensions/loader、theme 模块四个显式替身分别限制为预定解析、注入信任记录、受控 factory/cache 计数、JSON.parse。没有真实安装、设置持久化、jiti 生命周期或完整主题渲染。手写 .git metadata 不等于 git worktree 命令执行，目录读取失败也不等于 EACCES 权限实验。

2026-09-10 20:18:18 UTC 原运行 exit1，missing skill 错误期待 error，实际 warning，原失败栈保留；20:18:42 exit0 的 23 例、20:19:35 exit0 的 23 例是先后版本，不能加为 46 例。reviewed probe 强化 additional prompt 断言并改明来源优先名称。23 个场景涵盖构造、碰撞、CLI/额外路径、来源、可变 getter、重读、部分失败、信任/提示、扩展预载/冲突、theme/context/worktree。旧 B/C G-STAGE 缺当时 receipt 的失败仍保留，不修改为成功。主控本次没有重跑任何旧运行脚本，没有新的产品或上游行为测试。

本项 G-FILE 依据完整源阅读、边界解释与映射接受；当前 G-STAGE 结果由本次单次发布脚本分别记录候选态与主控接受态，并另保存实际 CLI stdout/stderr/exit code。任何目录、整项资源功能、目标大文件或 Stage1 总体验收均不从本项自动推出。

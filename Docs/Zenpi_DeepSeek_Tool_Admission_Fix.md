# DeepSeek 显式连接丢失全部工具：定位与修复

**日期**：2026-09-23
**症状**：用 DeepSeek API key 配置 profile 后，TUI / headless 会话**不向模型暴露任何工具**。
问模型"列出你可用的工具"，回答永远是 `NONE`。
后果：工具调用、文件读写、命令执行、**以及整个审批子系统**全部不可达。

---

## 1. 根因

不是 DeepSeek 的问题，也不是权限或网络问题——是**能力来源标记**的问题。

`src/providers/connection.rs` 在构造显式路由时，对每个"富能力"字段做一次来源检查：

```rust
// Legacy catalog fallback is intentionally permissive. Only explicit new
// connections require per-field catalog or user evidence for rich inputs.
for (field, supported) in [..., ("tools", &mut capabilities.tools), ...] {
    if !matches!(model.sources.get(field),
        Some(FieldSource::Builtin { .. } | FieldSource::UserOverride { .. })) {
        *supported = false;          // ← 这里
    }
}
```

`FieldSource` 有三个变体：`Builtin` / `UserOverride` / `Conservative`。
**只认前两个**，而 `Conservative` 被当作"没有证据"直接关掉。

DeepSeek 在 `src/providers/registry.rs` 的模型目录里**没有任何条目**，于是
`ModelRegistry::resolve("deepseek", "deepseek-flash")` 落到兜底构造器 `unknown()`——
它给**每一个字段**都标上 `FieldSource::Conservative`：

```
tools_source = Some(Conservative { version: "zenpi-models-2026-09-11.1" })
all_sources  = 全部 "Conservative"
```

于是 `tools` 被关 → 闸门往下传导：

```
connection.rs    tools = false  (route.capabilities())
backend.rs:1765  explicit_route → Some(route.capabilities())  → tools=false
core.rs:4606     model_tools = false
core.rs:4621     definitions.filter(model_tools && ...) → 空列表
core.rs:4732     CompletionRequest::new(..., &[])  → 模型看不到任何工具
```

**注意 `deepseek.rs` 自己的路由规则里 `tools` 是 `true`**（继承自
`for_wire_api(ChatCompletions)`）。也就是说 **provider 声明了支持工具，却被目录层的来源标记否决**——
两处对同一件事给出了相反的答案，而目录层赢了。

---

## 2. 修复

`src/providers/registry.rs`：给 DeepSeek 补上带 `Builtin` 来源的目录条目（+27 行）。

```rust
for id in ["deepseek-flash", "deepseek-reasoner"] {
    let source = FieldSource::Builtin {
        reference: "https://api-docs.deepseek.com/".into(),
        version: CATALOG_VERSION.into(),
    };
    let mut model = unknown("deepseek", id);
    model.context_window = 131_072;
    model.max_output_tokens = 65_536;
    for field in ["context_window", "max_output_tokens", "text", "tools", "streaming"] {
        model.sources.insert(field.into(), source.clone());
    }
    registry.entries.insert(("deepseek".into(), id.into()), model);
}
```

只声明 `tools` 等**确实核实过**的字段；`images`/`files`/`structured_output`/`reasoning_levels`
保持 `Conservative`，因为它们**没有**被核实——这正是该机制的意图，不要为了"看起来全"而放宽。

---

## 3. 验证

**修复前**：模型回答 `NONE`（"no function definitions were provided"）
**修复后**（真实 DeepSeek API，headless 端到端）：

```
edit_file, find, list_directory, load_skill, read_file, read_skill_resource,
read_tool_output, run_command, search_text, write_file
```

十个工具全部下发。

定位过程中用了一次**临时探针**（在 `backend.rs` 的 `model_capabilities` 两个分支各打一行，
在 `connection.rs` 的 `resolve` 之后打印 `FieldSource` 变体），它一次就分清了"走哪个分支"
和"来源是哪个变体"——**在两次猜测失败之后，探针比继续推理有效得多**。探针已在提交前完全移除。

回归：`--lib` 346 · `config` 33 · `explicit_runtime` 2 · `provider_admission` 12 ·
`provider_multimodal` 10，全绿。

---

## 4. 这次踩的坑（值得记住）

1. **两次修复尝试失败，方向都是错的**：先加目录条目（未生效，原因见下），
   再加用户覆盖 `model_overrides`（也没生效）。两次都在没有观测的情况下推理，
   直到加了探针才一次定位。**先观测，再修。**
2. **TUI 会恢复上一个会话**（不看 cwd），所以修复后在 TUI 里重测仍看到旧行为——
   它复用了修复前校验过的连接。判断"修没修好"要用 headless 开新会话，
   或用一个**干净 `ZENPI_HOME`**（只保留 `config.toml` + `auth.json`，
   删掉 `project-tabs.json` / `layout.json` / `sessions`）。
3. **同一个能力有两处声明**（provider 路由规则 vs 模型目录来源标记），
   它们可以互相矛盾。排查能力问题时**两处都要看**。

---

## 5. 还没做的

- **TUI 侧未复验**：修复的端到端验证是在 headless 完成的。TUI 走同一条
  `model_capabilities` 路径，理应一致，但**没有在 TUI 里确认过**。
  复验方法：干净 `ZENPI_HOME` + `zenpi` + 问模型列出工具。
- **审批流程仍未在 TUI 里真跑过**。这个 bug 之前一直挡着它，现在应该可以了。
- 其余未测流程：Shell 面板输入、Arch 面板、Ctrl-C / 双击 Ctrl-C、Ctrl-T 切项目、
  resize、退出。
- 修好之后值得考虑：**工具被关闭时给用户一个可见信号**。这次是全程静默的——
  配置文件写对了、doctor 报 `ready`、会话正常，但模型手上什么都没有。

---

## 6. 后续发现：TUI 仍然不下发工具（未修）

修完上一节之后做对照复验，发现**同一二进制、同一 `ZENPI_HOME`、同一工作区、同一提问**下：

| 路径 | 结果 |
|---|---|
| headless | ✅ 列出全部 10 个工具 |
| **TUI** | ❌ `no write_file tool is available to me in this session` |

并且 TUI 的对话里同时出现：

```
Shared project workspace could not be restored: working folder I/O:
No such file or directory (os error 2)
```

**所以 TUI 有一个 headless 没有的独立问题**，跟 DeepSeek 目录无关（那个已修，headless 侧已验证）。

### 排查线索

1. `core.rs` 里工具列表由两个条件共同决定：
   `model_tools`（后端能力）× `self.tools`（ToolRuntime 是否存在）。
   headless 同一后端下 `model_tools` 为真，所以嫌疑集中在 **TUI 路径上 `self.tools` 为 `None`**
   ——也就是 TUI 实际跑 turn 的那个 agent 没有 `set_tools`。
2. **`Shared project workspace could not be restored` 那行很可能是同一根因的线索**：
   项目工作区恢复失败 → owner 准备中断 → 会话落在一个没有 `ToolRuntime` 的 agent 上。
   本次测试时目标目录已被我删除，属于**受污染的测试环境**——
   **需要先用完全干净的工作区复现，确认这是真 bug 还是环境残留**，再动手。
3. 启动路径 `core.rs`（`set_tools_with_resources` 与 `RunMode::Tui` 分支）确实调用了 `set_tools`，
   所以要查的是**这个 agent 与 TUI 实际使用的是否同一个**——
   注意 `tui.rs` 里 `shared` 会被 `project_host.active(&state)` 重新指向 pool owner，
   两者若不一致就会出现"启动时设了、跑的时候没用上"。

### 复现方法

```sh
mkdir -p /tmp/zh /tmp/ws && chmod 700 /tmp/zh     # 0700 是凭据存储的硬要求
cp ~/.zenpi/config.toml ~/.zenpi/auth.json /tmp/zh/
cd /tmp/ws && git init -q && echo ok > README.md
tmux new-session -d -s zenpi -x 160 -y 45 -c /tmp/ws \
  "ZENPI_HOME=/tmp/zh TERM=xterm-256color ~/.cargo/bin/zenpi"
# 在 TUI 里问：List the exact names of the tools you have available
# headless 同参数对照，应当列出 10 个
```

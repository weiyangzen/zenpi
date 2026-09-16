#!/usr/bin/env python3
"""Generate same-prefix read-only Stage1 Gantt views from the active blueprint."""
from __future__ import annotations
import datetime as dt
import html
import json
import os
import re
import tempfile
from collections import Counter
from pathlib import Path
import validate_stage1_blueprint as v

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'Docs/stage_1_v3_pi_mono_gantt'
# Additional evidence never replaces the authoritative three checkbox states.
REPAIRS = {
    'ZS1-122': ('历史搜索Super误插入已修复：主控原1项失败→128项Rust通过，新debug真实PTY18项通过（草稿恢复/终端flags/退出/最终0HTTP）；普通输入与Ctrl-G既有修复保留，整项待验', 'history-super-integration-3.1.21', 'Docs/quality/stage1/ZS1-122/master-history-super-3.1.21/review.md'),
    'ZS1-128': ('矮窗口键盘焦点修复已合入：两项原失败→171项Rust通过，新debug真实PTY21项通过；放大/缩小、窄屏循环与焦点持久化已验证，BentoBox保持，整项待验', 'layout-viewport-integration-3.1.21', 'Docs/quality/stage1/ZS1-128/master-layout-viewport-3.1.21/review.md'),
    'ZS1-084': ('关闭输入终态修复已合入：主库原反例失败→新3项回归通过，相关共95项Rust通过；完整headless文件理解仍待验收', 'input-shutdown-release-3.1.21', 'Docs/quality/stage1/ZS1-132/master-input-closure-3.1.21/review.md'),
    'ZS1-120': ('新release5f1e5133真实项目PTY12项通过：顶部加号直开目录选择，名称/cwd绑定、shell目录隔离、迟到输出和草稿重启恢复', 'tui-busy-diff-integration-3.1.21', 'Docs/quality/stage1/ZS1-123/master-tui-busy-diff-3.1.21/review.md'),
    'ZS1-115': ('扩展超时夹具已复验：主库lib 96通过、0失败、5原有忽略；12个管道子进程场景通过，生产代码及超时门限不变', 'extension-fixture-integration', 'Docs/quality/stage1/ZS1-115/master-pipe-fixture-3.1.20/review.md'),
    'ZS1-125': ('正文查看器与审批预览窄屏emoji裁字已修复：原3失败→85项Rust通过，两个实际TestBackend画面保留末尾字符；原审批计时/提示修复保留，新PTY待验，整项待验', 'preview-grapheme-integration-3.1.21', 'Docs/quality/stage1/ZS1-125/master-preview-grapheme-3.1.21/review.md'),
    'ZS1-123': ('文件补全已显示部分/空结果及原因，矮屏保留可选文件：主控两原失败和一矮屏失败修复，162项回归+3独立lib、新debug实际PTY11项通过；完整模型review等仍待验', 'tui-file-completion-integration-3.1.21', 'Docs/quality/stage1/ZS1-123/master-file-completion-3.1.21/review.md'),
    'ZS1-117': ('新release5f1e5133预算7/8通过；首启1039.683291ms超过1000ms，三原样样本1039.68/38.87/42.97ms保留；旧d0cc失败仍留存，根因未确定', 'tui-busy-diff-integration-3.1.21', 'Docs/quality/stage1/ZS1-123/master-tui-busy-diff-3.1.21/review.md'),
    'ZS1-132': ('关闭前补齐输入终态及持久化重放：主库95项Rust通过，新release项目12项和BentoBox11项PTY通过；冷启动门禁失败，完整生命周期仍待验', 'input-shutdown-release-3.1.21', 'Docs/quality/stage1/ZS1-132/master-input-closure-3.1.21/review.md'),
    'ZS1-129': ('Esc边界修复已进入已测release6891df5d，真实kill/yank29项、4次HTTP、3个TUI通过；socket原反例及公共PTY前后均通过的反证保留', 'shutdown-current-3.1.21', 'Docs/quality/stage1/ZS1-132/master-shutdown-release-3.1.21/review.md'),
    'ZS1-126': ('会话列表滚动后误选与窄屏行错位已修复：主控2个原失败→143项Rust通过，边框/空白不改选择；既有异步浏览保留，BentoBox布局不变，整项待验', 'session-list-selection-integration-3.1.21', 'Docs/quality/stage1/ZS1-126/master-session-selection-3.1.21/review.md'),
    'ZS1-101': ('同步 TUI /input：实际 PTY 通过；输入回归 130 项', 'sync-input-integration', 'Docs/quality/stage1/ZS1-101/master-sync-input-3.1.19/review.md'),
    'ZS1-104': ('延迟摘要误提交已修复：实际HTTP四种deferred标志原误替换→拒绝并保留旧checkpoint，正常引用允许；118项顶层Rust通过，原手动输入修复保留，整项待验', 'summary-deferred-integration-3.1.21', 'Docs/quality/stage1/ZS1-104/master-summary-deferred-3.1.21/review.md'),
    'ZS1-106': ('分支取消组合：2 个 TUI、1 个 CLI、5 次 HTTP 通过；原会话保留、工具仅执行一次', 'branch106-select-cancel-integration', 'Docs/quality/stage1/ZS1-106/master-select-cancel-3.1.19/review.md'),
    'ZS1-108': ('Chat 终态取消：实际 before 失败、修复后 38 项通过', 'chat-terminal-cancel-integration', 'Docs/quality/stage1/ZS1-108/master-terminal-cancel-3.1.19/review.md'),
    'ZS1-109': ('Anthropic 终态取消：两反例修复，相关 52 项通过', 'anthropic-terminal-cancel-integration', 'Docs/quality/stage1/ZS1-109/master-terminal-cancel-3.1.19/review.md'),
    'ZS1-110': ('Gemini 终态取消：两反例修复，相关 52 项通过', 'gemini-terminal-cancel-integration', 'Docs/quality/stage1/ZS1-110/master-terminal-cancel-3.1.19/review.md'),
    'ZS1-111': ('长输出取消：主控 4942ms→4ms；97 项回归与命令强杀恢复通过', 'output111-io-integration', 'Docs/quality/stage1/ZS1-111/master-io-cancel-3.1.19/review.md'),
    'ZS1-112': ('二进制搜索修复：7 项CLI语义与取消恢复通过；深度探测因FIFO回归待修复', 'search112-public-integration', 'Docs/quality/stage1/ZS1-112/master-binary-end-3.1.19/review.md'),
    'ZS1-124': ('后台审批提醒已通过新release验证：4项背景PTY、14项审批与11项BentoBox检查通过；preview等完整边界待验', 'release-background-20260912', 'Docs/quality/stage1/ZS1-117/master-background-release-3.1.20/review.md'),
    'ZS1-130': ('外部编辑器已合入：60项交互与122项旧PTY通过；历史库测试失败尚未全部闭合，新release预算见117', 'external-editor-integration', 'Docs/quality/stage1/ZS1-130/master-external-editor-3.1.19/review.md'),
    'ZS1-131': ('读取器重置：真实 PTY、100 次循环和锁冲突测试通过', 'reader-reset-integration', 'Docs/quality/stage1/ZS1-131/master-reader-reset-3.1.19/review.md'),
}


BLOCKERS = {'ZS1-117': ('生产5f1e5133首启1039.683291ms超过1000ms，原三样本/exit1及旧d0cc失败保留。单次私有诊断：父启动至main首标记1096.025ms，main内40.610ms；入口前原因未分解，不替代生产预算，无重试或预热。', 'Docs/quality/stage1/ZS1-117/master-startup-markers-3.1.21/review.md')}


def atomic(path: Path, text: str) -> None:
    fd, name = tempfile.mkstemp(prefix='.stage1-gantt-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def main() -> None:
    bp = v.parse((ROOT / v.BLUEPRINT).read_text())
    sel = v.selector(ROOT, bp, v.BLUEPRINT)
    claims = v.read_json(ROOT / v.EVIDENCE / 'claims.json')['claims']
    owners = {c['item_id']: {'name': f"任务 {'ABC'[i]}", 'session': c['session'],
                           'running': c['runtime_status'] == 'live',
                           'note': c.get('runtime_note', '')} for i, c in enumerate(claims)}
    depth: dict[str, int] = {}
    def rank(key: str, visiting=frozenset()) -> int:
        if key in visiting:
            raise ValueError('cyclic dependency')
        if key not in depth:
            depth[key] = 1 + max((rank(d, visiting | {key}) for d in bp.items[key].depends), default=-1)
        return depth[key]
    titles = {}
    lines = {}
    for number, line in enumerate(bp.text.splitlines(), 1):
        match = re.match(r'^- \[[ _x]\] \*\*(ZS1-\d{3})\*\* — (.*?)；layer', line)
        if match:
            titles[match[1]], lines[match[1]] = match[2], number
    rows = []
    for key, item in bp.items.items():
        if key in bp.files:
            scope = bp.files[key].scope
            group = {'source': '源文件', 'target': '目标文件'}.get(scope, 'Codex 参考文件')
        elif key in bp.folders:
            group = '逐目录整合'
        else:
            group = 'TUI 交互' if key in {'ZS1-120','ZS1-121','ZS1-122','ZS1-123','ZS1-124','ZS1-125','ZS1-126','ZS1-127','ZS1-128','ZS1-129','ZS1-130','ZS1-131','ZS1-132'} else '产品与验收'
        note, evidence = '', ''
        if key in REPAIRS:
            label, directory, report = REPAIRS[key]
            verification = ROOT / '.ops/stage1_execution' / directory / 'master-verification.json'
            if verification.is_file() and (ROOT / report).is_file():
                json.loads(verification.read_text())
                note, evidence = label, report
        blocker, blocker_evidence = BLOCKERS.get(key, ('', ''))
        if blocker_evidence and not (ROOT / blocker_evidence).is_file():
            raise ValueError(f'missing blocker evidence: {blocker_evidence}')
        accepted_review = ''
        if item.state == '[x]':
            receipt = v.read_json(ROOT / v.EVIDENCE / 'receipts' / f'{key}.master.json')
            accepted_review = receipt['manual_review']['evidence']['path']
            v.artifact(ROOT, receipt['manual_review']['evidence'])
        rows.append(dict(id=key, title=titles[key], state=item.state, layer=item.layer,
                         group=group, depth=rank(key), depends=list(item.depends),
                         unmet=[d for d in item.depends if bp.items[d].state != '[x]'],
                         owned=list(item.owned_paths), owner=owners.get(key), note=note,
                         evidence=evidence or accepted_review, blocker=blocker,
                         blockerEvidence=blocker_evidence, line=lines[key]))
    counts = Counter(row['state'] for row in rows)
    now = dt.datetime.now(dt.timezone.utc).astimezone(dt.timezone(dt.timedelta(hours=8))).isoformat(timespec='seconds')
    data = dict(generated=now, version=bp.header['blueprint_version'], digest=bp.requirement,
                snapshot=bp.snapshot, counts=dict(counts), rows=rows,
                source=v.BLUEPRINT, maxDepth=max(depth.values()), run=sel['run_id'])
    assert len(rows) == len(bp.items) and len({row['id'] for row in rows}) == len(rows)
    md = [f'# zenpi Stage 1 v3 — pi-mono 差距执行 Gantt\n',
          f'> 更新时间：{now}。只读投影，唯一要求来源：[同名蓝图](stage_1_v3_pi_mono_blueprint.md)。[打开可筛选 Gantt](stage_1_v3_pi_mono_gantt.html)。\n',
          f'版本 `{data["version"]}` · 主控验收 `[x]` **{counts["[x]"]}/{len(rows)}** · worker 自测 `[_]` **{counts["[_]"]}** · 未完成 `[ ]` **{counts["[ ]"]}**。该比例按清单项计数，不是代码量或功能完成率。\n',
          '图中横轴为依赖层级；每项条宽相同，不表示工期，也不虚构日历日期。补充的局部修复验证不改变整项状态。每个文件、目录均独立列出。\n',
          '## 三个任务的当前认领\n', '| 任务 | 认领项 | 内容 |', '|---|---|---|']
    for row in rows:
        if row['owner']:
            status = '运行中' if row['owner']['running'] else '候选已交付，待主控复核'
            note = row['owner']['note'].replace('|', '∣')
            md.append(f'| {row["owner"]["name"]} | {row["id"]} | {row["title"]}（{status}）；{note} |')
    md += ['\n## 分组进度\n', '| 范围 | 总项 | `[x]` | `[_]` | `[ ]` |', '|---|---:|---:|---:|---:|']
    for group in dict.fromkeys(row['group'] for row in rows):
        group_rows = [row for row in rows if row['group'] == group]
        c = Counter(row['state'] for row in group_rows)
        md.append(f'| {group} | {len(group_rows)} | {c["[x]"]} | {c["[_]"]} | {c["[ ]"]} |')
    md += ['\n## 近期已合入的局部修复\n', '| ID | 当前验证 | 主控记录 |', '|---|---|---|']
    for row in rows:
        if row['note']:
            md.append(f'| {row["id"]} | {row["note"]}；整项仍 `{row["state"]}` | [证据]({Path(row["evidence"]).relative_to("Docs").as_posix()}) |')
    md += ['\n## 未通过的门禁\n', '| ID | 当前观察 | 证据 |', '|---|---|---|']
    for row in rows:
        if row['blocker']:
            md.append(f'| {row["id"]} | {row["blocker"]} | [记录]({Path(row["blockerEvidence"]).relative_to("Docs").as_posix()}) |')
    if not any(row['blocker'] for row in rows):
        md.append('\n本快照没有新增已归档的失败门禁。最近已测release及当前源码的验证范围见上表；历史失败、未复验的新增代码和未覆盖边界继续保留，不能推定全阶段通过。')
    md += ['\n## 全量依赖 Gantt\n', '```mermaid', 'gantt', '    title Stage 1 依赖层级投影（非日历工期）', '    dateFormat X', '    axisFormat %s']
    for group in dict.fromkeys(row['group'] for row in rows):
        md.append(f'    section {group}')
        for row in rows:
            if row['group'] != group:
                continue
            tag = 'done, ' if row['state'] == '[x]' else ('active, ' if row['owner'] and row['owner']['running'] else 'crit, ' if row['blocker'] else '')
            label = re.sub(r'[:#;\n]', ' ', row['title'])[:42]
            md.append(f'    {row["id"]} {label} :{tag}{row["id"]}, {row["depth"]}, 1s')
    md += ['```\n', '## 逐项监看\n', '| ID | 状态 | 范围 | 认领 | 依赖 | 验收未闭合的依赖 | 内容 |', '|---|---|---|---|---|---|---|']
    for row in rows:
        md.append(f'| {row["id"]} | `{row["state"]}` | {row["group"]} | {row["owner"]["name"] if row["owner"] else "—"} | {", ".join(row["depends"]) or "—"} | {", ".join(row["unmet"]) or "—"} | {row["title"].replace("|", "∣")} |')
    md += ['\n## 更新方式\n', '在项目根目录执行 `python3 tools/generate_stage1_gantt.py`，同时刷新 Markdown 与 HTML。每次主控整合或清单状态变化后重新生成。', f'\n源文件 SHA256：`{bp.snapshot}`\n\n要求 digest：`{bp.requirement}`\n']
    template = Path(__file__).with_name('stage1_gantt.html').read_text()
    payload = json.dumps(data, ensure_ascii=False).replace('<', '\\u003c').replace('&', '\\u0026')
    atomic(OUT.with_suffix('.md'), '\n'.join(md))
    atomic(OUT.with_suffix('.html'), template.replace('__GANTT_DATA__', payload))
    print(json.dumps(dict(generated=[str(OUT.with_suffix(s).relative_to(ROOT)) for s in ['.md','.html']], items=len(rows), counts=dict(counts)), ensure_ascii=False))


if __name__ == '__main__':
    main()

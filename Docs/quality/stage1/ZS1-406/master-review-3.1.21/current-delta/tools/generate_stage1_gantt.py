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
    'ZS1-084': ('显式版本选择修复已合入：旧生产55项检查有5项失败，新生产55项全部通过；长期回归及80项Rust通过，完整文件理解仍待验收', 'version-release-3.1.21', 'Docs/quality/stage1/ZS1-132/master-explicit-version-3.1.21/review.md'),
    'ZS1-120': ('已测release6891df5d真实项目PTY12项通过：顶部加号直接打开目录选择、路径绑定、真实shell目录隔离、迟到输出及重启草稿保持', 'shutdown-current-3.1.21', 'Docs/quality/stage1/ZS1-132/master-shutdown-release-3.1.21/review.md'),
    'ZS1-115': ('扩展超时夹具已复验：主库lib 96通过、0失败、5原有忽略；12个管道子进程场景通过，生产代码及超时门限不变', 'extension-fixture-integration', 'Docs/quality/stage1/ZS1-115/master-pipe-fixture-3.1.20/review.md'),
    'ZS1-125': ('单条回复跨8,192行后尾部与阅读锚点已修复：主库59项Rust、新PTY14项及原有16项检查通过；整项边界仍待验收', 'single-message-tail-integration', 'Docs/quality/stage1/ZS1-125/master-single-message-entrypoint-3.1.20/review.md'),
    'ZS1-123': ('busy /diff长期回归已合入，当前13项项目测试通过，旧产品保留12通过、新增1失败的反例；新目录/引号路径等既有修复保持，整项待验', 'busy-test-shell-3.1.21', 'Docs/quality/stage1/ZS1-132/master-busy-test-shell-3.1.21/review.md'),
    'ZS1-117': ('当前已测release92f95fbe的8项预算全部通过，固定首三样本981.79/46.35/37.69ms；首样本接近1000ms门限，未丢弃或重试；历史失败保留', 'version-release-3.1.21', 'Docs/quality/stage1/ZS1-132/master-explicit-version-3.1.21/review.md'),
    'ZS1-132': ('关闭后的锁等待已修复：实际API约5秒→1.28秒，主控53项embedding、68项Rust通过；已测release6891df5d预算及项目/编辑PTY通过，完整生命周期仍待验', 'shutdown-current-3.1.21', 'Docs/quality/stage1/ZS1-132/master-shutdown-release-3.1.21/review.md'),
    'ZS1-129': ('Esc边界修复已进入已测release6891df5d，真实kill/yank29项、4次HTTP、3个TUI通过；socket原反例及公共PTY前后均通过的反证保留', 'shutdown-current-3.1.21', 'Docs/quality/stage1/ZS1-132/master-shutdown-release-3.1.21/review.md'),
    'ZS1-126': ('会话浏览已移出输入线程：68项Rust、5项真实浏览/退出及26项项目分页与长回复回归通过；底层扫描取消和整项边界待验', 'async-session-browser-integration', 'Docs/quality/stage1/ZS1-126/master-async-browser-3.1.20/review.md'),
    'ZS1-101': ('同步 TUI /input：实际 PTY 通过；输入回归 130 项', 'sync-input-integration', 'Docs/quality/stage1/ZS1-101/master-sync-input-3.1.19/review.md'),
    'ZS1-104': ('手动压缩输入：58 项顶层回归 + 40 项输入回归通过', 'manual-compact-input-integration', 'Docs/quality/stage1/ZS1-104/master-manual-input-3.1.19/review.md'),
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


BLOCKERS = {}


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

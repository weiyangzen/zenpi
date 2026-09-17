#!/usr/bin/env python3
"""Same-prefix read-only Gantt projection for a Blueprint checklist.

Usage: python3 tools/gantt_projection.py [Docs/<name>_blueprint.md]

Blueprint `<dir>/<name>_blueprint.<ext>` maps to `<dir>/<name>_gantt.<ext>`
(markdown + html). The projection carries no mutable checkboxes and is never
parsed as authority; every checklist id appears exactly once.
"""
from __future__ import annotations

import datetime as dt
import hashlib
import html
import json
import os
import re
import sys
import tempfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ITEM_RE = re.compile(
    r"^- \[([ _x])\] \*\*(ZS1-\d{3})\*\* — (.*?)；layer `(L\d)` \| Depends: (.*?) \|"
)


def atomic(path: Path, text: str) -> None:
    fd, name = tempfile.mkstemp(prefix=".gantt-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as handle:
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def parse(text: str):
    items = {}
    for line in text.splitlines():
        match = ITEM_RE.match(line)
        if match:
            state, iid, title, layer, depends = match.groups()
            items[iid] = {
                "state": f"[{state}]",
                "title": title.strip(),
                "layer": layer,
                "depends": [d.strip() for d in depends.split(",") if d.strip().startswith("ZS1-")],
            }
    return items


def depth(items, iid, visiting=frozenset()):
    if iid in visiting or not items[iid]["depends"]:
        return 1
    return 1 + max(depth(items, d, visiting | {iid}) for d in items[iid]["depends"] if d in items)


def main(argv):
    blueprint = Path(argv[0]) if argv else ROOT / "Docs/stage_1_v3_pi_mono_blueprint.md"
    if not blueprint.is_absolute():
        blueprint = ROOT / blueprint
    stem = blueprint.stem
    if stem.lower().endswith("blueprint"):
        stem = stem[: -len("blueprint")] + "gantt"
    else:
        stem = stem + "_gantt"
    md_path = blueprint.with_name(stem + ".md")
    html_path = blueprint.with_name(stem + ".html")

    raw = blueprint.read_bytes()
    items = parse(raw.decode())
    depths = {iid: depth(items, iid) for iid in items}
    max_depth = max(depths.values(), default=1)
    counts = Counter(i["state"] for i in items.values())
    now = dt.datetime.now(dt.timezone(dt.timedelta(hours=8))).isoformat(timespec="seconds")
    digest = hashlib.sha256(raw).hexdigest()
    order = list(items)

    lines = [
        f"# {stem} — Blueprint Gantt\n",
        f"> 更新时间：{now}。只读投影，唯一要求来源：[同名蓝图]({blueprint.name})。\n",
        f"- blueprint digest: `{digest}`\n- 生成时间：{now}\n",
        f"清单项 `[x]` **{counts.get('[x]', 0)}/{len(items)}** · `[_]` **{counts.get('[_]', 0)}** · "
        f"`[ ]` **{counts.get('[ ]', 0)}**。按清单项计数，不是代码完成率。\n",
        "横轴为依赖层级；条宽相同，不表示工期，也不虚构日历。\n",
        "## 分组进度\n",
        "| 层级 | 总项 | `[x]` | `[_]` | `[ ]` |",
        "|---|---:|---:|---:|---:|",
    ]
    for layer in sorted({i["layer"] for i in items.values()}):
        group = [i for i in items.values() if i["layer"] == layer]
        c = Counter(g["state"] for g in group)
        lines.append(f"| {layer} | {len(group)} | {c.get('[x]', 0)} | {c.get('[_]', 0)} | {c.get('[ ]', 0)} |")
    lines += ["", "## 依赖层级 Gantt\n", "```mermaid", "gantt",
              "    title Blueprint 依赖层级投影（非日历工期）", "    dateFormat X", "    axisFormat %s"]
    for layer in sorted({i["layer"] for i in items.values()}):
        lines.append(f"    section {layer}")
        for iid in order:
            if items[iid]["layer"] != layer:
                continue
            mark = {"[x]": "done, ", "[_]": "active, ", "[ ]": ""}[items[iid]["state"]]
            label = items[iid]["title"][:40].replace(":", "：")
            lines.append(f"    {iid} {label} :{mark}{iid}, {depths[iid]}, 1s")
    lines += ["```", "", "## 监控索引（每项恰好一次）\n",
              "| ID | 状态 | 层级 | 依赖 | 未满足依赖 | 标题 |",
              "|---|---|---|---|---|---|"]
    for iid in order:
        item = items[iid]
        unmet = [d for d in item["depends"] if items[d]["state"] != "[x]"]
        lines.append(
            f"| {iid} | {item['state'].replace(' ', '·')} | {item['layer']} | "
            f"{','.join(item['depends']) or '—'} | {','.join(unmet) or '—'} | {html.escape(item['title'])} |"
        )
    lines.append("")
    atomic(md_path, "\n".join(lines))

    bars = []
    for iid in order:
        left = (depths[iid] - 1) * (100.0 / max(max_depth, 1))
        cls = {"[x]": "done", "[_]": "self", "[ ]": "todo"}[items[iid]["state"]]
        bars.append(
            f'<div class="bar {cls}" style="left:{left:.2f}%;width:{100.0/max(max_depth,1):.2f}%" '
            f'title="{html.escape(iid + " " + items[iid]["title"])}">{iid}</div>'
        )
    rows = "".join(
        f'<tr class="{ {"[x]":"done","[_]":"self","[ ]":"todo"}[items[i]["state"]] }">'
        f'<td>{i}</td><td>{html.escape(items[i]["state"])}</td><td>{items[i]["layer"]}</td>'
        f'<td>{html.escape(items[i]["title"])}</td>'
        f'<td>{html.escape(",".join(items[i]["depends"]) or "—")}</td></tr>'
        for i in order
    )
    atomic(html_path, f"""<!DOCTYPE html><html lang="zh"><head><meta charset="utf-8">
<title>{stem}</title><style>
body{{font-family:system-ui,sans-serif;margin:24px;background:#0f1115;color:#e6e6e6}}
.board{{position:relative;margin:18px 0;padding:8px 0;border-top:1px solid #333;border-bottom:1px solid #333}}
.bar{{position:relative;height:16px;margin:2px 0;border-radius:3px;font-size:10px;line-height:16px;overflow:hidden;white-space:nowrap;padding-left:3px;box-sizing:border-box}}
.todo{{background:#3b4252;color:#cbd5e1}}.self{{background:#8a6d1f;color:#fff}}.done{{background:#1f7a4d;color:#fff}}
table{{border-collapse:collapse;width:100%;font-size:12px}}th,td{{border:1px solid #333;padding:4px 6px;text-align:left}}
th{{background:#1b1f27}}</style></head><body>
<h1>{html.escape(stem)}（只读投影）</h1>
<p>blueprint digest {digest}<br>generated {now}</p>
<p>`[x]` {counts.get('[x]', 0)}/{len(items)} · `[_]` {counts.get('[_]', 0)} · `[ ]` {counts.get('[ ]', 0)}</p>
<div class="board">{''.join(bars)}</div>
<table><thead><tr><th>ID</th><th>状态</th><th>层级</th><th>标题</th><th>依赖</th></tr></thead>
<tbody>{rows}</tbody></table></body></html>
""")
    print(json.dumps({"gantt_md": str(md_path.relative_to(ROOT)), "gantt_html": str(html_path.relative_to(ROOT)),
                      "items": len(items), "counts": dict(counts), "blueprint_digest": digest}, ensure_ascii=False))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

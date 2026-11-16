#!/usr/bin/env python3
"""Builds a local HTML review report from system-detect / nav-sim output.

Usage: report.py <run-dir> <nav-dir> <groundtruth.json> <out.html>

<run-dir>/<doc>/result.json + pNNN.png come from scripts/batch.sh, <nav-dir>/<name>/nav.json +
view PNGs from scripts/nav_examples.sh. groundtruth.json holds a manual judgement per page
(see its "_legend"). Image links are relative to the report, so keep everything in one folder
tree. Never commit the output: it shows (possibly copyrighted) score pages.
"""

import html
import json
import os
import sys
from collections import OrderedDict, defaultdict

run_dir, nav_dir, gt_path, out_path = sys.argv[1:5]
base = os.path.dirname(os.path.abspath(out_path))
rel = lambda p: os.path.relpath(p, base)
gt = json.load(open(gt_path))
legend, gdocs = gt["_legend"], gt["docs"]
VERDICTS = ["ok", "ok-minor", "nomusic", "cut", "missed", "merged", "fallback"]

docs = []
for name in sorted(os.listdir(run_dir)):
    f = os.path.join(run_dir, name, "result.json")
    if os.path.exists(f):
        docs.append((name, json.load(open(f))))


def verdict(name, page):
    g = gdocs.get(name, {})
    v = g.get("pages", {}).get(str(page))
    if v:
        return v[0], v[1]
    note = g.get("pages", {}).get("_all", "")
    return g.get("default", "?"), note


E = html.escape
rows_cat = OrderedDict()
rows_doc = []
page_rows = []
timing = []
for name, d in docs:
    cat = gdocs.get(name, {}).get("category", "?")
    counts = defaultdict(int)
    flagged_ok = 0  # judged fine but confidence below threshold (lost benefit, not an error)
    unflagged_bad = 0  # judged wrong but not flagged (an error the user would see)
    rms, dms = [], []
    for p in d["pages"]:
        r = p["result"]
        v, note = verdict(name, p["page"])
        counts[v] += 1
        if r["fallback"] and v in ("ok", "ok-minor"):
            flagged_ok += 1
        if not r["fallback"] and v in ("cut", "missed", "merged"):
            unflagged_bad += 1
        rms.append(p["render_ms"])
        dms.append(p["detect_ms"])
        systems = [g for g in r["regions"] if g["kind"] == "system"]
        page_rows.append(
            f"<tr class='v-{E(v)}'><td>{E(name)}<br>p. {p['page']}</td>"
            f"<td><a href='{E(rel(os.path.join(run_dir, name, 'p%03d.png' % p['page'])))}'>"
            f"<img loading=lazy src='{E(rel(os.path.join(run_dir, name, 'p%03d.png' % p['page'])))}'></a></td>"
            f"<td>{len(systems)}</td><td>{' / '.join(str(g['staves']) for g in systems) or '–'}</td>"
            f"<td>{r['confidence']:.2f}{' <b>fallback</b>' if r['fallback'] else ''}</td>"
            f"<td>{E('; '.join(r['issues']))}</td>"
            f"<td>{p['render_ms']:.0f} + {p['detect_ms']:.0f}</td>"
            f"<td><b>{E(v)}</b> {E(note)}</td></tr>"
        )
    n = len(d["pages"])
    music = n - counts["nomusic"]
    good = counts["ok"] + counts["ok-minor"]
    rows_doc.append((cat, name, n, music, counts, flagged_ok, unflagged_bad))
    c = rows_cat.setdefault(cat, defaultdict(int))
    c["pages"] += n
    c["music"] += music
    c["good"] += good
    c["flagged_ok"] += flagged_ok
    c["unflagged_bad"] += unflagged_bad
    for k in VERDICTS:
        c[k] += counts[k]
    timing.append((name, sum(p["dpi"] for p in d["pages"]) / n, n, sum(rms) / n, max(rms), sum(dms) / n, max(dms)))

out = []
w = out.append
w("<!doctype html><meta charset=utf-8><title>System detection report</title>")
w("""<style>
:root{--bg:#fff;--fg:#1d1d1f;--mut:#666;--line:#ddd;--ok:#e7f6ea;--minor:#f3f8e4;--bad:#fde8e8;--fb:#fff4dc;--nm:#f2f2f2}
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){--bg:#141414;--fg:#eee;--mut:#aaa;--line:#333;--ok:#18301d;--minor:#262f17;--bad:#3a1b1b;--fb:#3a2f14;--nm:#222}}
body{background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,sans-serif;margin:0 auto;max-width:1300px;padding:16px}
table{border-collapse:collapse;margin:8px 0 24px;width:100%}td,th{border-bottom:1px solid var(--line);padding:4px 6px;text-align:left;vertical-align:top}
th{font-weight:600}td.n,th.n{text-align:right}img{width:240px;display:block}
.v-ok{background:var(--ok)}.v-ok-minor{background:var(--minor)}.v-cut,.v-missed,.v-merged{background:var(--bad)}.v-fallback{background:var(--fb)}.v-nomusic{background:var(--nm)}
.views{display:flex;flex-wrap:wrap;gap:8px}.views figure{margin:0}.views img{width:auto;max-width:100%;height:220px;border:1px solid var(--line)}
figcaption{color:var(--mut);font-size:12px}p.mut{color:var(--mut)}
</style>""")
w("<h1>Music-system detection — prototype report</h1>")
w("<p class=mut>Local review only (pages of copyrighted scores). Overlay colours: red = staff lines, "
  "orange lines = tablature, blue/green boxes = systems (number = staves), orange box = header, "
  "grey = footer, magenta = cut lines (dashed = cut crosses ink), red frame = fallback to full page.</p>")

w("<h2>Accuracy per category (manual judgement of every overlay)</h2><table><tr><th>Category</th>"
  "<th class=n>pages</th><th class=n>music pages</th><th class=n>correct</th><th class=n>of which minor</th>"
  "<th class=n>cut wrong</th><th class=n>missed</th><th class=n>merged</th><th class=n>flagged fallback</th>"
  "<th class=n>correct but flagged</th><th class=n>wrong, not flagged</th></tr>")
for cat, c in rows_cat.items():
    pct = 100 * c["good"] / c["music"] if c["music"] else 0
    w(f"<tr><td>{E(cat)}</td><td class=n>{c['pages']}</td><td class=n>{c['music']}</td>"
      f"<td class=n>{c['good']} ({pct:.0f} %)</td><td class=n>{c['ok-minor']}</td><td class=n>{c['cut']}</td>"
      f"<td class=n>{c['missed']}</td><td class=n>{c['merged']}</td><td class=n>{c['fallback']}</td>"
      f"<td class=n>{c['flagged_ok']}</td><td class=n>{c['unflagged_bad']}</td></tr>")
w("</table>")
w("<p class=mut>" + "<br>".join(f"<b>{E(k)}</b>: {E(v)}" for k, v in legend.items()) + "</p>")

w("<h2>Per document</h2><table><tr><th>Category</th><th>Document</th><th class=n>pages</th>"
  "<th class=n>music</th>" + "".join(f"<th class=n>{v}</th>" for v in VERDICTS) + "</tr>")
for cat, name, n, music, counts, fo, ub in rows_doc:
    w(f"<tr><td>{E(cat)}</td><td>{E(name)}</td><td class=n>{n}</td><td class=n>{music}</td>"
      + "".join(f"<td class=n>{counts[v] or ''}</td>" for v in VERDICTS) + "</tr>")
w("</table>")

w("<h2>Timing (this machine, one core, release build)</h2><table><tr><th>Document</th><th class=n>dpi</th>"
  "<th class=n>pages</th><th class=n>render mean / max ms</th><th class=n>detect mean / max ms</th></tr>")
for name, dpi, n, rm, rx, dm, dx in timing:
    w(f"<tr><td>{E(name)}</td><td class=n>{dpi:.0f}</td><td class=n>{n}</td><td class=n>{rm:.0f} / {rx:.0f}</td>"
      f"<td class=n>{dm:.0f} / {dx:.0f}</td></tr>")
w("</table>")

w("<h2>Navigation simulation</h2><p class=mut>Screens: 8\" 1280×800 px at 189 ppi (172×108 mm) and "
  "10.5\" 2224×1668 px at 264 ppi (214×160 mm), landscape and portrait. Staff height = 4 line distances. "
  "'today' = full page fitted to the screen. Views show whole systems cropped to the content width; "
  "the next view starts with the last system of the previous one. 'below min' = views where even the "
  "width-limited scale cannot reach the minimum staff height (content wider than the screen at that size).</p>")
for name in sorted(os.listdir(nav_dir)):
    f = os.path.join(nav_dir, name, "nav.json")
    if not os.path.exists(f):
        continue
    d = json.load(open(f))
    w(f"<h3>{E(name)}</h3><table><tr><th>screen</th><th class=n>min staff</th><th class=n>pages</th>"
      "<th class=n>systems</th><th class=n>views</th><th class=n>systems/view</th><th class=n>below min</th>"
      "<th class=n>staff today</th><th class=n>staff in views</th><th class=n>magnification</th></tr>")
    for c in d["combos"]:
        w(f"<tr><td>{E(c['screen'])}</td><td class=n>{c['min_staff_mm']:.0f} mm</td><td class=n>{c['pages']}</td>"
          f"<td class=n>{c['systems']}</td><td class=n>{c['views']}</td><td class=n>{c['mean_systems_per_view']:.1f}</td>"
          f"<td class=n>{c['views_below_min']}</td><td class=n>{c['staff_mm_page_fit']:.1f} mm</td>"
          f"<td class=n>{c['staff_mm_views']:.1f} mm</td><td class=n>×{c['magnification']:.2f}</td></tr>")
    w("</table><div class=views>")
    for img in sorted(x for x in os.listdir(os.path.join(nav_dir, name)) if x.endswith(".png")):
        w(f"<figure><a href='{E(rel(os.path.join(nav_dir, name, img)))}'><img loading=lazy "
          f"src='{E(rel(os.path.join(nav_dir, name, img)))}'></a><figcaption>{E(img)}</figcaption></figure>")
    w("</div>")

w("<h2>All pages</h2><table><tr><th>Page</th><th>Overlay</th><th>systems</th><th>staves per system</th>"
  "<th>confidence</th><th>issues</th><th>render + detect ms</th><th>judgement</th></tr>")
out.extend(page_rows)
w("</table>")
open(out_path, "w").write("\n".join(out))
print(f"wrote {out_path}: {len(page_rows)} pages, {len(docs)} documents")

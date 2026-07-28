"""Render the board as a self-contained HTML page for leanzero.net.

Scale's leaderboard shape — a card per capability, ranked rows, error bars — with the things that
board does NOT have and this one must: a row that failed to DELIVER is coloured as such rather than
dropped, entrants whose intervals overlap are marked TIED instead of silently ordered, and the
refusals travel with the page instead of living in a methodology note nobody opens.

Self-contained by construction: no CDN, no webfont URL, no external asset. The Artifact CSP blocks
all three, and a silently-fallen-back font is worse than a system stack chosen on purpose.
"""

from __future__ import annotations

import argparse
import html
import json
from pathlib import Path
from typing import Dict, List

BOARD = Path(__file__).resolve().parents[1]

CSS = """
:root{
  --ink:#0B0E14; --panel:#141924; --line:#232A38; --hair:#1B2130;
  --text:#E8ECF4; --muted:#8E9AB0; --faint:#5E6B82;
  --fleet:#FFB020; --base:#4C7BF3; --fail:#FF4D4D; --good:#00C48C;
  --track:#1E2533;
}
@media (prefers-color-scheme: light){
  :root{ --ink:#F7F8FA; --panel:#FFFFFF; --line:#E2E6EE; --hair:#EDF0F5;
         --text:#111726; --muted:#5B6577; --faint:#8B95A7; --track:#E8ECF3; }
}
:root[data-theme="dark"]{ --ink:#0B0E14; --panel:#141924; --line:#232A38; --hair:#1B2130;
  --text:#E8ECF4; --muted:#8E9AB0; --faint:#5E6B82; --track:#1E2533; }
:root[data-theme="light"]{ --ink:#F7F8FA; --panel:#FFFFFF; --line:#E2E6EE; --hair:#EDF0F5;
  --text:#111726; --muted:#5B6577; --faint:#8B95A7; --track:#E8ECF3; }

*{box-sizing:border-box}
body{margin:0;background:var(--ink);color:var(--text);
  font-family:ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;
  font-size:15px;line-height:1.5;-webkit-font-smoothing:antialiased}
.wrap{max-width:1180px;margin:0 auto;padding:48px 24px 96px}

.mast{display:flex;flex-wrap:wrap;align-items:flex-end;gap:20px;
  padding-bottom:22px;border-bottom:2px solid var(--line);margin-bottom:14px}
.mast h1{margin:0;font-size:clamp(28px,4vw,42px);font-weight:800;letter-spacing:-.028em;
  text-wrap:balance}
.mast .sub{color:var(--muted);max-width:60ch;margin:6px 0 0;font-size:15px}
.stamp{margin-left:auto;font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:11.5px;
  color:var(--faint);text-align:right;line-height:1.7;font-variant-numeric:tabular-nums}

.strip{display:flex;flex-wrap:wrap;gap:8px;margin:18px 0 34px}
.chip{font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:11.5px;letter-spacing:.04em;
  text-transform:uppercase;padding:6px 11px;border-radius:3px;background:var(--panel);
  border:1px solid var(--line);color:var(--muted);font-variant-numeric:tabular-nums}
.chip b{color:var(--text);font-weight:700}
.chip.alert{background:var(--fail);border-color:var(--fail);color:#fff}
.chip.alert b{color:#fff}

.grid{display:grid;gap:20px;grid-template-columns:repeat(auto-fit,minmax(340px,1fr))}
.card{background:var(--panel);border:1px solid var(--line);border-radius:6px;padding:22px 22px 18px}
.card h2{margin:0;font-size:13px;font-weight:800;letter-spacing:.1em;text-transform:uppercase}
.card .q{margin:7px 0 20px;color:var(--muted);font-size:13.5px;min-height:2.6em}

.row{display:grid;grid-template-columns:22px 1fr auto;gap:10px;align-items:baseline;
  padding:9px 0;border-top:1px solid var(--hair)}
.row:first-of-type{border-top:0}
.rank{font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:12px;color:var(--faint);
  font-variant-numeric:tabular-nums}
.name{font-size:13.5px;font-weight:600;overflow-wrap:anywhere}
.tag{font-size:10.5px;font-weight:700;letter-spacing:.06em;text-transform:uppercase;
  margin-left:7px;padding:2px 6px;border-radius:3px;vertical-align:1px}
.tag.fleet{background:var(--fleet);color:#241a00}
.tag.dead{background:var(--fail);color:#fff}
.val{font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:13.5px;font-weight:700;
  font-variant-numeric:tabular-nums;white-space:nowrap}
.val .ci{color:var(--faint);font-weight:400;font-size:11.5px}

.meter{grid-column:2/4;position:relative;height:9px;border-radius:2px;background:var(--track);
  margin-top:2px;overflow:visible}
.fill{position:absolute;left:0;top:0;bottom:0;border-radius:2px;background:var(--base)}
.fill.fleet{background:var(--fleet)}
.fill.dead{background:var(--fail)}
.whisk{position:absolute;top:-3px;bottom:-3px;border-left:2px solid var(--text);
  border-right:2px solid var(--text);opacity:.55}
.n{grid-column:2/4;font-family:ui-monospace,"SF Mono",Menlo,monospace;font-size:11px;
  color:var(--faint);font-variant-numeric:tabular-nums;margin-top:3px}

.note{margin-top:16px;padding-top:13px;border-top:1px solid var(--hair);
  font-size:12px;color:var(--muted)}
.note b{color:var(--text)}
.tied{color:var(--fleet);font-weight:700;letter-spacing:.04em}

.refuse{margin-top:44px;border:1px solid var(--line);border-radius:6px;background:var(--panel);
  padding:24px}
.refuse h3{margin:0 0 4px;font-size:13px;font-weight:800;letter-spacing:.1em;text-transform:uppercase}
.refuse p{margin:0 0 16px;color:var(--muted);font-size:13.5px;max-width:70ch}
.refuse ul{margin:0;padding:0;list-style:none;display:grid;gap:9px}
.refuse li{display:grid;grid-template-columns:auto 1fr;gap:11px;align-items:start;
  font-size:13.5px;color:var(--muted)}
.refuse li span{color:var(--fail);font-weight:800}
"""


def _bar(row: Dict) -> str:
    dead = row["pct"] == 0 or row.get("crashed")
    kind = "dead" if dead else ("fleet" if not row["baseline"] else "")
    lo, hi = max(0.0, row["lo"]), min(100.0, row["hi"])
    whisker = (f'<i class="whisk" style="left:{lo:.2f}%;width:{max(hi - lo, 0.6):.2f}%"></i>'
               if hi > lo else "")
    return (f'<div class="meter"><i class="fill {kind}" style="width:{max(row["pct"], 0.8):.2f}%">'
            f'</i>{whisker}</div>')


def _row(row: Dict) -> str:
    half = (row["hi"] - row["lo"]) / 2
    tag = ""
    if row.get("crashed"):
        tag = '<span class="tag dead">did not finish</span>'
    elif not row["baseline"]:
        tag = '<span class="tag fleet">your fleet</span>'
    detail = f'{row["passes"]}/{row["denom"]} · n={row["n"]} · {row["median_secs"]:.0f}s median'
    return (
        f'<div class="row"><div class="rank">{row["rank"]}</div>'
        f'<div class="name">{html.escape(row["label"])}{tag}</div>'
        f'<div class="val">{row["pct"]:.1f}<span class="ci"> ±{half:.1f}</span></div>'
        f'{_bar(row)}<div class="n">{detail}</div></div>')


def _card(card: Dict) -> str:
    rows = "".join(_row(r) for r in card["rows"])
    tied = [r["label"] for r in card["rows"]
            if sum(1 for x in card["rows"] if x["rank"] == r["rank"]) > 1]
    note = ""
    if tied:
        note = (f'<div class="note"><span class="tied">TIED</span> — intervals overlap for '
                f'{html.escape(", ".join(tied))}, so this card refuses to order them.</div>')
    return (f'<section class="card"><h2>{html.escape(card["title"])}</h2>'
            f'<p class="q">{html.escape(card["question"])}</p>{rows}{note}</section>')


def render(payload: Dict) -> str:
    cards = "".join(_card(c) for c in payload["cards"])
    integrity = payload["integrity"]
    zeroed = len(integrity["scored_zero_for_not_finishing"])
    chips = [
        f'<span class="chip"><b>{integrity["episodes"]}</b> episodes</span>',
        f'<span class="chip"><b>{len(payload["cards"])}</b> capabilities</span>',
        f'<span class="chip"><b>{integrity["tampered"]}</b> tampered</span>',
    ]
    if zeroed:
        chips.append(f'<span class="chip alert"><b>{zeroed}</b> scored 0 — did not finish</span>')
    refusals = "".join(f'<li><span>&times;</span><div>{html.escape(r)}</div></li>'
                       for r in payload["refusals"])
    return f"""<style>{CSS}</style>
<div class="wrap">
  <header class="mast">
    <div>
      <h1>goose Agent Board</h1>
      <p class="sub">Coding agents graded by running what they build, on your own hardware,
        against frontier models on identical frozen tasks.</p>
    </div>
    <div class="stamp">
      board {html.escape(payload["board_version"])}<br>
      build {html.escape(payload["build_sha"])}<br>
      profile {html.escape(payload["profile"]["sha256"][:16])}…<br>
      {payload["profile"]["files"]} graded files
    </div>
  </header>
  <div class="strip">{"".join(chips)}</div>
  <div class="grid">{cards}</div>
  <section class="refuse">
    <h3>What this board refuses to do</h3>
    <p>Every one of these costs the page a cleaner-looking result. They are the reason a number
      here survives someone re-running it.</p>
    <ul>{refusals}</ul>
  </section>
</div>"""


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--export", type=Path, default=BOARD / "runs/board-export.json")
    ap.add_argument("--out", type=Path, default=BOARD / "runs/board.html")
    args = ap.parse_args()

    payload = json.loads(args.export.read_text())
    args.out.write_text(render(payload))
    print(f"wrote {args.out}  ({len(payload['cards'])} cards, "
          f"{payload['integrity']['episodes']} episodes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

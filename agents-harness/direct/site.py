"""The seed campaign's static site: one index of the 50 seed tweets and one
report page per tweet, from the files `run_direct.py` writes under
`<out>/<post_id>/`. Every file may be missing or partial: a page renders
what exists and says what is pending. Stdlib only; charts are inline SVG.

    python3 -I site.py --seeds docs/design/fixtures/seed-posts.tsv \
        --out /srv/absolute/direct/out --site /srv/absolute/site \
        [--mirror /srv/absolute/data/seed]

X content is the owner's to read: every page says `noindex`, nothing is
republished, and nothing here trades.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import html
import json
import math
import os
import re
from pathlib import Path
from typing import Any

BASE = 1_000_000.0
RISK = 7_500.0

CSS = """
:root{--bg:#ffffff;--fg:#16181d;--muted:#5b6170;--line:#d9dce3;--card:#f6f7f9;--accent:#1f5fbf;
--pos:#1a7f37;--neg:#c0362c;--warn:#a15c00;--oos:rgba(161,92,0,.10);--chart:#1f5fbf;--dd:#c0362c}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){--bg:#111317;--fg:#e6e8ec;--muted:#9aa1ad;
--line:#2b2f37;--card:#181b21;--accent:#6ea2ff;--pos:#4cc173;--neg:#ff7b70;--warn:#f0a43a;--oos:rgba(240,164,58,.12);
--chart:#6ea2ff;--dd:#ff7b70}}
:root[data-theme="dark"]{--bg:#111317;--fg:#e6e8ec;--muted:#9aa1ad;--line:#2b2f37;--card:#181b21;--accent:#6ea2ff;
--pos:#4cc173;--neg:#ff7b70;--warn:#f0a43a;--oos:rgba(240,164,58,.12);--chart:#6ea2ff;--dd:#ff7b70}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);
font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}
main{max-width:1100px;margin:0 auto;padding:16px}a{color:var(--accent)}h1{font-size:1.5rem;margin:.4em 0}
h2{font-size:1.15rem;margin:1.6em 0 .5em;border-bottom:1px solid var(--line);padding-bottom:.2em}
h3{font-size:1rem;margin:1.1em 0 .4em}.muted{color:var(--muted)}.card{background:var(--card);border:1px solid var(--line);
border-radius:8px;padding:12px 14px;margin:10px 0}.tweet{white-space:pre-wrap;word-wrap:break-word}
.imgs{display:flex;flex-wrap:wrap;gap:8px;margin-top:8px}.imgs img{max-width:100%;max-height:340px;border-radius:6px;border:1px solid var(--line)}
table{border-collapse:collapse;width:100%;margin:6px 0;font-size:.92rem}th,td{border-bottom:1px solid var(--line);
padding:4px 8px;text-align:left;vertical-align:top}th{font-weight:600}td.n,th.n{text-align:right;font-variant-numeric:tabular-nums}
.wrap{overflow-x:auto}pre{background:var(--card);border:1px solid var(--line);border-radius:6px;padding:10px;
overflow-x:auto;font-size:.85rem;white-space:pre}.pill{display:inline-block;padding:1px 8px;border-radius:10px;
font-size:.8rem;border:1px solid var(--line)}.full{color:var(--pos);border-color:var(--pos)}.no{color:var(--neg);border-color:var(--neg)}
.pend{color:var(--warn);border-color:var(--warn)}.fires{color:var(--neg);font-weight:600}.clear{color:var(--pos)}
.nc{color:var(--muted)}.counts{display:flex;flex-wrap:wrap;gap:10px}.counts .card{margin:0;min-width:140px}
.counts b{font-size:1.4rem;display:block}svg{max-width:100%;height:auto;display:block}
.chart text{fill:var(--muted);font-size:11px}.ax{stroke:var(--line)}
"""


def esc(x: Any) -> str:
    return html.escape("" if x is None else str(x))


def load(p: Path) -> Any:
    try:
        return json.loads(p.read_text("utf-8"))
    except (OSError, ValueError):
        return None


def parse_time(v: Any) -> dt.datetime | None:
    """ISO-8601 or nanoseconds since the epoch, UTC."""
    if v is None or v == "":
        return None
    if isinstance(v, (int, float)) or (isinstance(v, str) and v.lstrip("-").isdigit()):
        n = int(v)
        if abs(n) > 10**15:
            n //= 1_000_000_000
        elif abs(n) > 10**12:
            n //= 1000
        return dt.datetime.fromtimestamp(n, dt.UTC)
    s = str(v).strip().replace("Z", "+00:00")
    try:
        t = dt.datetime.fromisoformat(s)
    except ValueError:
        try:
            t = dt.datetime.strptime(s[:10], "%Y-%m-%d")
        except ValueError:
            return None
    return t if t.tzinfo else t.replace(tzinfo=dt.UTC)


def day(v: Any) -> str:
    t = parse_time(v)
    return t.strftime("%Y-%m-%d %H:%M UTC") if t else esc(v)


def num(v: Any, kind: str = "f") -> str:
    if v is None or (isinstance(v, float) and (math.isnan(v) or math.isinf(v))):
        return "—"
    try:
        x = float(v)
    except (TypeError, ValueError):
        return esc(v)
    if kind == "pct":
        return f"{x * 100:.2f}%"
    if kind == "int":
        return f"{int(round(x)):,}"
    if kind == "usd":
        return f"${x:,.0f}"
    return f"{x:.3f}"


def page(title: str, body: str) -> str:
    return (
        '<!doctype html><html lang="en"><head><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width, initial-scale=1">'
        '<meta name="robots" content="noindex, nofollow">'
        f"<title>{esc(title)}</title><style>{CSS}</style></head><body><main>{body}</main></body></html>"
    )


# ---------------------------------------------------------------- inputs


def campaign_of(path: Path) -> str:
    """The campaign a post list names: `seed-posts.tsv` the seed, `campaign-2-posts.tsv` campaign 2."""
    stem = path.name.removesuffix(".tsv").removesuffix("-posts")
    return "seed" if stem == "seed" else stem.replace("-", " ")


def seeds(paths: str | Path) -> list[dict[str, str]]:
    """The posts of one list or of several, comma-separated (the seed and campaign 2)."""
    rows, seen = [], set()
    for path in [Path(x) for x in str(paths).split(",") if x]:
        if not path.exists():
            continue
        for line in path.read_text("utf-8").splitlines():
            if not line.strip() or line.startswith("#") or line.startswith("created_utc"):
                continue
            c = line.split("\t")
            if len(c) >= 4 and c[2] not in seen:
                seen.add(c[2])
                rows.append({"created": c[0], "handle": c[1], "id": c[2], "url": c[3], "campaign": campaign_of(path)})
    return rows


class Post:
    """Everything known of one seed post."""

    def __init__(self, seed: dict[str, str], out: Path, mirror: Path | None) -> None:
        self.seed = seed
        self.id = seed["id"]
        d = out / self.id
        self.dir = d
        self.status = load(d / "status.json") or {}
        self.tweet = load(d / "tweet.json") or {}
        self.triage = load(d / "triage.json") or {}
        self.hyp = load(d / "hypothesis.json") or {}
        self.analysis = load(d / "analysis.json") or {}
        self.quant = load(d / "quant.json") or {}
        self.report = load(d / "report.json") or {}
        self.reveal = load(d / "reveal.json") or {}
        check = load(d / "check.json") or {}
        self.check: dict[str, Any] = check.get("plan_run") or check
        # A translation-check plan is never the result, wherever it was written.
        if self.report and not is_development_report(self.report):
            self.check, self.report, self.reveal, self.nav_ok = self.report, {}, {}, False
        else:
            self.nav_ok = True
        self.verdict = load(d / "verdict.json") or {}
        self.nav = read_nav(d / "nav.csv") if self.nav_ok and self.report else []
        self.mirror: dict[str, Any] = {}
        if mirror is not None:
            m = load(mirror / f"{self.id}.json") or {}
            self.mirror = m.get("tweet") or {}

    # -- the tweet
    @property
    def handle(self) -> str:
        a = self.tweet.get("author") or {}
        return str(a.get("handle") or self.mirror.get("author", {}).get("screen_name") or self.seed["handle"])

    @property
    def name(self) -> str:
        return str(self.mirror.get("author", {}).get("name") or "")

    @property
    def created(self) -> str:
        return str(self.tweet.get("fixed_at") or self.seed["created"])

    @property
    def text(self) -> str:
        if self.tweet.get("source_deleted"):
            return ""
        return str(self.tweet.get("text") or self.mirror.get("text") or "")

    @property
    def images(self) -> list[str]:
        imgs = list(self.tweet.get("images") or [])
        if not imgs:
            for m in (self.mirror.get("media") or {}).get("all") or []:
                u = m.get("url") if m.get("type") == "photo" else m.get("thumbnail_url")
                if u:
                    imgs.append(u)
        return [str(u) for u in imgs if str(u).startswith("https://")]

    # -- the outcome
    def base(self) -> dict[str, Any] | None:
        for o in self.report.get("outcomes") or []:
            if o.get("step") == "base" and o.get("status") == "completed":
                return o
        for o in self.report.get("outcomes") or []:
            if o.get("status") == "completed" and o.get("kind") == "trial":
                return o
        return None

    def oos(self) -> dict[str, Any] | None:
        for src in (self.reveal, self.report):
            for o in (src or {}).get("outcomes") or []:
                if o.get("kind") == "reveal" and o.get("status") == "completed":
                    return o
        return None

    def state(self) -> tuple[str, str]:
        """(full | no | pend, the line the index shows)."""
        if self.base() is not None:
            return "full", "full backtest"
        if self.check and str(self.status.get("state") or "") not in ("running", "queued", ""):
            reason = self.quant.get("reason") or self.status.get("reason") or "only the translation check ran"
            return "no", f"backtest not completed: {reason}"
        st = str(self.status.get("state") or "")
        reason = self.status.get("reason") or self.quant.get("reason") or self.analysis.get("reason") or ""
        if st in ("not_testable", "no_hypothesis", "no_data", "failed", "could_not_check", "no_decisions", "budget", "error"):
            label = {"no_hypothesis": "no testable idea", "not_testable": "not testable"}.get(st, st.replace("_", " "))
            return "no", f"{label}: {reason}" if reason else label
        if st in ("full", "done") and not self.report:
            return "pend", "backtest finished, report pending"
        return "pend", f"pending ({self.status.get('stage') or 'queued'})"


def is_development_report(run: dict[str, Any]) -> bool:
    """A development plan's report: not a `check_` plan, with a grid and the out-of-sample reveal."""
    nf = str(run.get("normal_form") or "")
    return not str(run.get("plan") or "").startswith("check_") and bool(re.search(r"\bgrid\s+\w+", nf)) and bool(re.search(r"\breveal\s+\w+\s+of\b", nf))


def check_html(check: dict[str, Any]) -> str:
    """The translation check, labelled as such: one market, a short window; never a result."""
    outs = [o for o in check.get("outcomes") or [] if o.get("status") == "completed"]
    rows = []
    for o in outs:
        t = o.get("trades") or {}
        w = o.get("window") or []
        rows.append(
            f"<tr><td>{esc(o.get('step'))}</td><td>{esc(' .. '.join(str(x)[:10] for x in w))}</td><td>{esc((o.get('trading') or {}).get('decisions'))}</td>"
            f"<td>{esc(t.get('trades'))}</td><td>{num(t.get('net_profit'), 'usd') if t.get('net_profit') is not None else '—'}</td></tr>"
        )
    table = "<div class=wrap><table><tr><th>Run</th><th>Window</th><th>Decisions</th><th>Trades</th><th>Net profit</th></tr>" + "".join(rows) + "</table></div>" if rows else ""
    return (
        "<h3>Translation check (one market, short window)</h3>"
        f"<p class=muted>Plan <code>{esc(check.get('plan'))}</code>: the quant's check that the program trades as written, "
        "recomputed by hand. It is not a backtest and its numbers are not a result.</p>" + table
    )


def read_nav(p: Path) -> list[tuple[dt.datetime, float]]:
    out: list[tuple[dt.datetime, float]] = []
    try:
        with p.open(newline="") as f:
            for row in csv.reader(f):
                if len(row) < 2:
                    continue
                t = parse_time(row[0])
                try:
                    v = float(row[1])
                except ValueError:
                    continue
                if t is not None and math.isfinite(v):
                    out.append((t, v))
    except OSError:
        pass
    out.sort()
    return out


# ---------------------------------------------------------------- metrics


def metrics_of(o: dict[str, Any] | None) -> dict[str, Any]:
    if not o:
        return {}
    r, t = o.get("returns") or {}, o.get("trades") or {}
    return {
        "return": t.get("annual_return"),
        "volatility": r.get("volatility"),
        "sharpe": r.get("sharpe"),
        "max_drawdown": t.get("max_drawdown_base", r.get("max_drawdown")),
        "mar": t.get("mar"),
        "trades": t.get("trades"),
        "profit_factor": t.get("profit_factor"),
        "hit_rate": t.get("hit_rate"),
    }


def metrics_of_nav(nav: list[tuple[dt.datetime, float]], periods: float = 252.0) -> dict[str, Any]:
    """Full-sample figures on the fixed base from the NAV (trades come from the runs)."""
    if len(nav) < 3:
        return {}
    rets = [(nav[i][1] - nav[i - 1][1]) / BASE for i in range(1, len(nav))]
    mean = sum(rets) / len(rets)
    var = sum((x - mean) ** 2 for x in rets) / max(1, len(rets) - 1)
    vol = math.sqrt(var) * math.sqrt(periods)
    ann = mean * periods
    peak, mdd = nav[0][1], 0.0
    for _, v in nav:
        peak = max(peak, v)
        mdd = max(mdd, (peak - v) / BASE)
    return {"return": ann, "volatility": vol, "sharpe": ann / vol if vol > 0 else None, "max_drawdown": mdd, "mar": ann / mdd if mdd > 0 else None}


ROWS = [
    ("return", "Average return a year (fixed base)", "pct"),
    ("volatility", "Volatility", "pct"),
    ("sharpe", "Sharpe", "f"),
    ("max_drawdown", "Max drawdown (of $1M)", "pct"),
    ("mar", "MAR", "f"),
    ("trades", "Trades", "int"),
    ("profit_factor", "Profit factor", "f"),
    ("hit_rate", "Hit rate", "pct"),
]


def metrics_table(full: dict[str, Any], ins: dict[str, Any], oos: dict[str, Any]) -> str:
    rows = "".join(
        f"<tr><th>{esc(label)}</th><td class=n>{num(full.get(k), kind)}</td><td class=n>{num(ins.get(k), kind)}</td><td class=n>{num(oos.get(k), kind)}</td></tr>" for k, label, kind in ROWS
    )
    return f"<div class=wrap><table><tr><th></th><th class=n>Full</th><th class=n>In-sample</th><th class=n>Out-of-sample</th></tr>{rows}</table></div>"


# ---------------------------------------------------------------- charts


def chart(series: list[tuple[dt.datetime, float]], color: str, label: str, oos_from: dt.datetime | None, pct: bool) -> str:
    if len(series) < 2:
        return f"<p class=muted>No {esc(label.lower())} yet.</p>"
    w, h, pl, pr, pt, pb = 860, 240, 64, 12, 12, 26
    t0, t1 = series[0][0].timestamp(), series[-1][0].timestamp()
    vs = [v for _, v in series]
    lo, hi = min(vs), max(vs)
    if hi == lo:
        hi, lo = hi + 1, lo - 1
    span = t1 - t0 or 1.0

    def x(t: float) -> float:
        return pl + (t - t0) / span * (w - pl - pr)

    def y(v: float) -> float:
        return pt + (hi - v) / (hi - lo) * (h - pt - pb)

    step = max(1, len(series) // 1200)
    pts = " ".join(f"{x(t.timestamp()):.1f},{y(v):.1f}" for t, v in series[::step] + [series[-1]])
    shade = ""
    if oos_from is not None and t0 < oos_from.timestamp() < t1:
        xo = x(oos_from.timestamp())
        shade = f'<rect x="{xo:.1f}" y="{pt}" width="{w - pr - xo:.1f}" height="{h - pt - pb}" fill="var(--oos)"/><text x="{xo + 4:.1f}" y="{pt + 12}">out-of-sample</text>'
    ticks = ""
    for i in range(5):
        v = lo + (hi - lo) * i / 4
        lab = f"{v * 100:.1f}%" if pct else f"${v / 1e6:.2f}M"
        ticks += f'<line class=ax x1="{pl}" x2="{w - pr}" y1="{y(v):.1f}" y2="{y(v):.1f}"/><text x="{pl - 6}" y="{y(v) + 4:.1f}" text-anchor="end">{lab}</text>'
    years = ""
    for yr in range(series[0][0].year + 1, series[-1][0].year + 1):
        ts = dt.datetime(yr, 1, 1, tzinfo=dt.UTC).timestamp()
        years += f'<text x="{x(ts):.1f}" y="{h - 8}" text-anchor="middle">{yr}</text>'
    return f'<svg class=chart viewBox="0 0 {w} {h}" role="img" aria-label="{esc(label)}">{shade}{ticks}{years}<polyline fill="none" stroke="{color}" stroke-width="1.6" points="{pts}"/></svg>'


def drawdown(nav: list[tuple[dt.datetime, float]]) -> list[tuple[dt.datetime, float]]:
    out, peak = [], -math.inf
    for t, v in nav:
        peak = max(peak, v)
        out.append((t, (v - peak) / BASE))
    return out


# ---------------------------------------------------------------- red flags


def red_flags(p: Post, ins: dict[str, Any] | None, oos: dict[str, Any] | None) -> list[tuple[str, str, str, str]]:
    """(flag, state ∈ fires|clear|nc, what was found, threshold) for guideline §12."""
    out: list[tuple[str, str, str, str]] = []
    t = (ins or {}).get("trades") or {}
    r = (ins or {}).get("returns") or {}
    to, ro = (oos or {}).get("trades") or {}, (oos or {}).get("returns") or {}
    ppy = float((ins or {}).get("periods_per_year") or 252)

    def add(name: str, fires: bool | None, found: str, thr: str) -> None:
        out.append((name, "nc" if fires is None else ("fires" if fires else "clear"), found, thr))

    n = t.get("trades")
    oos_n, oos_years = to.get("trades"), None
    w = (oos or {}).get("window")
    if isinstance(w, list) and len(w) == 2:
        a, b = parse_time(w[0]), parse_time(w[1])
        if a and b and b > a:
            oos_years = (b - a).days / 365.25
    per_year = (oos_n / oos_years) if (oos_n is not None and oos_years) else None
    if n is None:
        add("Too few trades", None, "no run", "< 200 in total, or < 10 a year out of sample")
    else:
        add(
            "Too few trades",
            n < 200 or (per_year is not None and per_year < 10),
            f"{n} in sample" + (f", {per_year:.1f} a year out of sample" if per_year is not None else ""),
            "< 200, or < 10 a year OOS",
        )

    ents = t.get("by_entity") or []
    total = sum(float(e.get("net_profit") or 0) for e in ents)
    top = t.get("top_market_share")
    if top is None and ents and total > 0:
        top = max(float(e.get("net_profit") or 0) for e in ents) / total
    prof = t.get("profitable_market_share")
    if top is None and prof is None:
        add("One market carries it", None, "no contribution table", "one market > 40 % of net profit, or profitable in < 50 % of markets")
    else:
        fire = (top is not None and top > 0.4) or (prof is not None and prof < 0.5)
        add("One market carries it", fire, f"top market {num(top, 'pct')}, profitable in {num(prof, 'pct')} of markets", "> 40 % / < 50 %")

    pb = t.get("profit_before_1990_share")
    add("Profits before 1990", None if pb is None else pb > 0.5, "the store starts in 2010" if pb is None else num(pb, "pct"), "most net profit before 1990")

    pf, atc = t.get("profit_factor"), t.get("avg_trade_to_cost")
    if pf is None:
        add("Thin edge", None, "no run", "PF < 1.2, or average trade < 2 × round-turn cost")
    else:
        add("Thin edge", pf < 1.2 or (atc is not None and atc < 2), f"PF {num(pf)}, average trade {num(atc)} × cost", "PF < 1.2 / < 2 × cost")

    si, so = r.get("sharpe"), ro.get("sharpe")
    mi, mo = t.get("mar"), to.get("mar")
    if si is None or so is None:
        add("Collapses out of sample", None, "no reveal yet" if si is not None else "no run", "IS Sharpe > 1 → OOS < 0.3; MAR sign flips")
    else:
        flip = mi is not None and mo is not None and (mi > 0) != (mo > 0)
        add("Collapses out of sample", (si > 1 and so < 0.3) or flip, f"Sharpe {num(si)} → {num(so)}, MAR {num(mi)} → {num(mo)}", "IS > 1 → OOS < 0.3; sign flip")

    dd, dur = t.get("max_drawdown_base"), r.get("max_drawdown_duration")
    if dd is None:
        add("Deep or long drawdown", None, "no run", "max drawdown > 30 % of capital; duration in years")
    else:
        yrs = (dur / ppy) if dur is not None else None
        add("Deep or long drawdown", dd > 0.30 or (yrs is not None and yrs >= 1), f"{num(dd, 'pct')} of $1M, {num(yrs)} years", "> 30 %; ≥ 1 year")

    g = (p.report.get("grids") or [{}])[0] if p.report.get("grids") else None
    if not g:
        add("Parameter cliff", None, "no grid", "a single good cell; PBO")
    else:
        pbo = (g.get("pbo") or {}).get("pbo")
        stab = (g.get("surface") or {}).get("stability")
        add("Parameter cliff", (pbo is not None and pbo > 0.5) or (stab is not None and stab < 0.5), f"PBO {num(pbo)}, neighbours within 10 %: {num(stab, 'pct')}", "PBO > 0.5; a lone cell")

    ll, sk = t.get("largest_loss"), t.get("trade_skew")
    if ll is None:
        add("Fat left tail", None, "no run", "largest loss against the $7,500 risk; trade skew")
    else:
        add("Fat left tail", ll > 3 * RISK or (sk is not None and sk < -1), f"largest loss {num(ll, 'usd')} ({ll / RISK:.1f} × risk), skew {num(sk)}", "> 3 × risk; skew < −1")

    intraday = p.quant.get("intraday_on_daily")
    add(
        "Intraday logic on daily bars",
        None if intraday is None else bool(intraday),
        esc(p.quant.get("intraday_note") or ("from the quant's analysis" if intraday is not None else "the quant did not say")),
        "§2",
    )

    au = author_numbers(p)
    differs = [k for k in ("compounding", "costs", "sizing", "instrument") if au.get(k) not in (None, "", "unknown")]
    add(
        "Author's setup differs",
        None if not au else bool(differs),
        ", ".join(f"{k}: {au[k]}" for k in differs) or ("no author numbers" if not au else "nothing stated"),
        "compounding, no costs, equal weight, stocks",
    )
    return out


def author_numbers(p: Post) -> dict[str, Any]:
    for src in (p.verdict.get("author"), p.verdict.get("author_reported"), p.hyp.get("author_reported")):
        if isinstance(src, str):
            try:
                src = json.loads(src)
            except ValueError:
                src = None
        if isinstance(src, dict) and src:
            return src
    return {}


# ---------------------------------------------------------------- pages


def list_html(items: Any) -> str:
    if not items:
        return "<p class=muted>None stated.</p>"
    if isinstance(items, str):
        items = [items]
    return "<ul>" + "".join(f"<li>{esc(i)}</li>" for i in items) + "</ul>"


def kv(rows: list[tuple[str, Any]]) -> str:
    return "<div class=wrap><table>" + "".join(f"<tr><th>{esc(k)}</th><td>{v}</td></tr>" for k, v in rows if v not in (None, "", [])) + "</table></div>"


def grid_table(p: Post) -> str:
    g = (p.report.get("grids") or [None])[0]
    outs = p.report.get("outcomes") or []
    if not g:
        return "<p class=muted>No sensitivity grid in the report.</p>"
    cells: list[tuple[dict[str, str], float | None]] = []
    for i in g.get("runs") or []:
        if 0 <= i < len(outs):
            o = outs[i]
            cells.append(({k: v for k, v in (o.get("point") or [])}, (o.get("returns") or {}).get("sharpe")))
    if not cells:
        return "<p class=muted>The grid has no completed point.</p>"
    names = list(cells[0][0])
    vals = [v for _, v in cells if v is not None]
    lo, hi = (min(vals), max(vals)) if vals else (0.0, 1.0)

    def shade(v: float | None) -> str:
        if v is None or hi == lo:
            return ""
        a = 0.08 + 0.5 * (v - lo) / (hi - lo)
        return f' style="background:rgba(31,127,55,{a:.2f})"'

    pbo = (g.get("pbo") or {}).get("pbo")
    head = f"<p class=muted>Objective: Sharpe in sample. PBO {num(pbo)}; best cell index {esc(g.get('best'))}.</p>"
    if len(names) == 2:
        a, b = names
        av = sorted({c[0][a] for c in cells}, key=lambda s: float(s) if s.replace(".", "", 1).lstrip("-").isdigit() else s)
        bv = sorted({c[0][b] for c in cells}, key=lambda s: float(s) if s.replace(".", "", 1).lstrip("-").isdigit() else s)
        look = {(c[0][a], c[0][b]): c[1] for c in cells}
        rows = "".join(f"<tr><th>{esc(a)} = {esc(x)}</th>" + "".join(f"<td class=n{shade(look.get((x, y)))}>{num(look.get((x, y)))}</td>" for y in bv) + "</tr>" for x in av)
        return head + f"<div class=wrap><table><tr><th></th>{''.join(f'<th class=n>{esc(b)} = {esc(y)}</th>' for y in bv)}</tr>{rows}</table></div>"
    rows = "".join(f"<tr><td>{esc(', '.join(f'{k} = {v}' for k, v in pt.items()))}</td><td class=n{shade(v)}>{num(v)}</td></tr>" for pt, v in cells)
    return head + f"<div class=wrap><table><tr><th>Point</th><th class=n>Sharpe</th></tr>{rows}</table></div>"


def contribution(p: Post, base: dict[str, Any] | None) -> str:
    t = (base or {}).get("trades") or {}
    names = p.quant.get("entity_names") or p.report.get("entity_names") or {}
    ents = sorted(t.get("by_entity") or [], key=lambda e: -abs(float(e.get("net_profit") or 0)))
    years = t.get("by_year") or []
    if not ents and not years:
        return "<p class=muted>No contribution tables in the report.</p>"
    # A share of net profit means something only when the net is positive.
    net = sum(float(e.get("net_profit") or 0) for e in ents)
    total = net if net > 0 else None
    m = "".join(
        f"<tr><td>{esc(names.get(str(e.get('entity')), e.get('entity')))}</td><td class=n>{num(e.get('net_profit'), 'usd')}</td>"
        f"<td class=n>{num(e.get('share') if e.get('share') is not None else (float(e.get('net_profit') or 0) / total if total else None), 'pct')}</td>"
        f"<td class=n>{num(e.get('trades'), 'int')}</td><td class=n>{num(e.get('hit_rate'), 'pct')}</td></tr>"
        for e in ents[:25]
    )
    y = "".join(f"<tr><td>{esc(r.get('year'))}</td><td class=n>{num(r.get('net_profit'), 'usd')}</td><td class=n>{num(r.get('trades'), 'int')}</td></tr>" for r in years)
    more = f"<p class=muted>{len(ents) - 25} more markets not shown.</p>" if len(ents) > 25 else ""
    return (
        "<h3>By market</h3><div class=wrap><table><tr><th>Market</th><th class=n>Net profit</th><th class=n>Share</th><th class=n>Trades</th><th class=n>Hit rate</th></tr>"
        + m
        + "</table></div>"
        + more
        + "<h3>By year</h3><div class=wrap><table><tr><th>Year</th><th class=n>Net profit</th><th class=n>Trades</th></tr>"
        + y
        + "</table></div>"
    )


def verdict_html(p: Post) -> str:
    v = p.verdict
    if not v:
        return "<p class=muted>Pending.</p>"
    parts = v.get("parts") or v.get("conclusion") or v.get("rationale")
    out = ""
    if v.get("verdict"):
        out += f"<p><b>{esc(v['verdict'])}</b></p>"
    if isinstance(parts, dict):
        for k, txt in parts.items():
            out += f"<h3>{esc(k)}</h3><p class=tweet>{esc(txt)}</p>"
    elif isinstance(parts, list):
        for txt in parts:
            out += f"<p class=tweet>{esc(txt)}</p>"
    elif parts:
        out += f"<p class=tweet>{esc(parts)}</p>"
    return out or f"<pre>{esc(json.dumps(v, indent=2))}</pre>"


def author_vs_ours(p: Post, full: dict[str, Any], ins: dict[str, Any], oos: dict[str, Any]) -> str:
    au = author_numbers(p)
    if not au:
        return "<p class=muted>The author reported no numbers.</p>"
    pairs = [
        ("Sharpe", au.get("sharpe"), full.get("sharpe") or ins.get("sharpe"), "f"),
        ("Max drawdown", au.get("max_drawdown"), full.get("max_drawdown") or ins.get("max_drawdown"), "pct"),
        ("Return a year", au.get("cagr") or au.get("annual_return"), full.get("return") or ins.get("return"), "pct"),
        ("Trades", au.get("trades"), ins.get("trades"), "int"),
        ("Win rate", au.get("win_rate"), ins.get("hit_rate"), "pct"),
        ("Profit factor", au.get("profit_factor"), ins.get("profit_factor"), "f"),
    ]
    rows = "".join(f"<tr><th>{esc(k)}</th><td class=n>{num(a, kind)}</td><td class=n>{num(o, kind)}</td></tr>" for k, a, o, kind in pairs if a is not None)
    rest = {k: v for k, v in au.items() if k not in ("sharpe", "max_drawdown", "cagr", "annual_return", "trades", "win_rate", "profit_factor")}
    extra = kv([(k, esc(v)) for k, v in rest.items()]) if rest else ""
    return (f"<div class=wrap><table><tr><th></th><th class=n>Author</th><th class=n>Ours</th></tr>{rows}</table></div>" if rows else "") + extra


def tweet_page(p: Post, built: str) -> str:
    kind, line = p.state()
    pill = {"full": "full", "no": "no", "pend": "pend"}[kind]
    b = f'<p><a href="../index.html">← all tweets</a></p><h1>@{esc(p.handle)} · {esc(day(p.created))}</h1>'
    b += f'<p><span class="pill {pill}">{esc(line)}</span></p>'
    # The tweet.
    imgs = "".join(f'<img src="{esc(u)}" alt="image attached to the post" loading="lazy" referrerpolicy="no-referrer">' for u in p.images)
    text = p.text or ("<i>source deleted</i>" if p.tweet.get("source_deleted") else "<i>text pending</i>")
    b += "<h2>The tweet</h2><div class=card>"
    b += f'<p class=muted>{esc(p.name)} @{esc(p.handle)} · fixed at {esc(day(p.created))} (H) · <a href="{esc(p.seed["url"])}" rel="noreferrer">on X</a></p>'
    b += f"<div class=tweet>{text if not p.text else esc(p.text)}</div>"
    if imgs:
        b += f"<div class=imgs>{imgs}</div>"
    b += "</div>"
    if p.triage:
        b += f"<p class=muted>Triage: {esc(p.triage.get('verdict'))} (score {num(p.triage.get('score'))}); topics {esc(', '.join(p.triage.get('topics') or []))}.</p>"
    # The idea.
    b += "<h2>The idea</h2>"
    if p.hyp:
        b += f"<div class=card><b>{esc(p.hyp.get('statement'))}</b></div>"
        b += kv(
            [
                ("Universe", esc(p.hyp.get("universe"))),
                ("Signal", f"<span class=tweet>{esc(p.hyp.get('signal'))}</span>"),
                ("Horizon", esc(p.hyp.get("horizon"))),
                ("Direction", esc(p.hyp.get("direction"))),
                ("Data needs", esc(", ".join(p.hyp.get("data_needs") or []))),
                ("Confidence", num(p.hyp.get("confidence"))),
            ]
        )
    elif p.analysis:
        b += f"<p>No testable idea: {esc(p.analysis.get('reason') or p.analysis.get('notes'))}</p>"
    else:
        b += "<p class=muted>Pending.</p>"
    b += "<h3>Assumptions</h3>" + list_html(p.quant.get("assumptions") or p.hyp.get("assumptions"))
    # The analysis.
    b += "<h2>The analysis</h2>"
    if p.quant:
        testable = p.quant.get("testable")
        roots = p.quant.get("roots") or []
        b += kv(
            [
                ("Testable", "yes" if testable else ("no" if testable is not None else esc(p.quant.get("status")))),
                ("On which futures", esc(", ".join(roots)) or "—"),
                ("Reason", esc(p.quant.get("reason"))),
                ("Program hash", f"<code>{esc(p.quant.get('program_hash'))}</code>"),
                ("Plan hash", f"<code>{esc(p.quant.get('plan_hash'))}</code>"),
                ("Lineage", f"<code>{esc(p.quant.get('lineage'))}</code>"),
            ]
        )
    else:
        b += f"<p class=muted>{esc(p.status.get('reason') or 'Pending.')}</p>"
    prog = p.quant.get("program") or p.quant.get("program_text")
    plan = p.quant.get("plan") or p.quant.get("plan_text")
    if prog:
        b += f"<h3>The program (futures_house)</h3><pre>{esc(prog)}</pre>"
    if plan:
        b += f"<h3>The plan</h3><pre>{esc(plan)}</pre>"
    # The results.
    base, oos = p.base(), p.oos()
    ins_m, oos_m = metrics_of(base), metrics_of(oos)
    full_m = metrics_of_nav(p.nav, float((base or {}).get("periods_per_year") or 252))
    oos_from = None
    emb = (p.report.get("embargo") or {}).get("intervals") or []
    if emb and emb[0]:
        oos_from = parse_time(emb[0][0])
    b += "<h2>The results</h2>"
    if base is None:
        reason = p.quant.get("reason") or p.status.get("reason") or line
        b += f"<p><b>Backtest not completed:</b> {esc(reason)}</p>"
        if p.check:
            b += check_html(p.check)
    else:
        b += "<h3>Equity on the fixed $1M base</h3>" + chart(p.nav, "var(--chart)", "Equity curve", oos_from, False)
        b += "<h3>Drawdown, % of $1M</h3>" + chart(drawdown(p.nav), "var(--dd)", "Drawdown", oos_from, True)
        b += "<h3>Full, in-sample, out-of-sample</h3>" + metrics_table(full_m, ins_m, oos_m)
        if not full_m:
            b += "<p class=muted>Full: the NAV is pending.</p>"
        if not oos_m:
            b += "<p class=muted>Out of sample: the reveal is pending.</p>"
        dsr = p.report.get("dsr") or {}
        if dsr:
            b += f"<p class=muted>Deflated Sharpe {num(dsr.get('dsr'))} over the lineage's {esc(dsr.get('trials'))} trials.</p>"
        b += "<h2>Contribution</h2>" + contribution(p, base)
        b += "<h2>Sensitivity</h2>" + grid_table(p)
    b += "<h2>Red flags (guideline §12)</h2>"
    flags = red_flags(p, base, oos)
    b += (
        "<div class=wrap><table><tr><th>Flag</th><th>State</th><th>Found</th><th>Threshold</th></tr>"
        + "".join(
            f"<tr><td>{esc(n)}</td><td class={s}>{ {'fires': 'fires', 'clear': 'clear', 'nc': 'not computable'}[s] }</td><td>{f}</td><td class=muted>{esc(thr)}</td></tr>" for n, s, f, thr in flags
        )
        + "</table></div>"
    )
    b += "<h2>The author's numbers beside ours</h2>" + author_vs_ours(p, full_m, ins_m, oos_m)
    b += "<h2>Verdict</h2>" + verdict_html(p)
    b += f'<p class=muted style="margin-top:2em">Built {esc(built)}. Post {esc(p.id)}. Private: X content for the owner only. Nothing trades.</p>'
    return page(f"@{p.handle} · {p.id}", b)


def verdict_line(p: Post) -> str:
    v = p.verdict.get("verdict") if p.verdict else None
    return str(v) if v else ""


def index_page(posts: list[Post], built: str) -> str:
    kinds = [p.state()[0] for p in posts]
    n_full, n_no, n_pend = kinds.count("full"), kinds.count("no"), kinds.count("pend")
    camps = sorted({p.seed.get("campaign", "seed") for p in posts}, key=lambda c: (c != "seed", c))
    b = f"<h1>{esc(' and '.join(camps).capitalize())}: {len(posts)} tweets</h1>"
    b += f"<p class=muted>Built {esc(built)}. Each tweet links to its report. Plans: window ending at the tweet's creation time H, hold-out trailing 20 %, budget money 0, the futures_house preset. Private: X content for the owner only; nothing trades.</p>"
    b += (
        f"<div class=counts><div class=card><b>{n_full}</b>full backtests</div><div class=card><b>{n_no}</b>not testable</div>"
        f"<div class=card><b>{n_pend}</b>pending</div><div class=card><b>{len(posts)}</b>tweets</div></div>"
    )
    rows = ""
    for p in sorted(posts, key=lambda q: parse_time(q.created) or dt.datetime.min.replace(tzinfo=dt.UTC), reverse=True):
        kind, line = p.state()
        excerpt = (p.text[:160] + "…") if len(p.text) > 160 else p.text
        rows += (
            f'<tr><td>{esc(day(p.created)[:10])}</td><td>{esc(p.seed.get("campaign", "seed"))}</td><td>@{esc(p.handle)}</td><td><a href="t/{esc(p.id)}.html">{esc(excerpt) or "<i>text pending</i>"}</a></td>'
            f'<td><span class="pill {kind}">{esc(line[:140])}</span></td><td>{esc(verdict_line(p)[:200])}</td></tr>'
        )
    b += f"<div class=wrap><table><tr><th>Date</th><th>Campaign</th><th>Author</th><th>Tweet</th><th>Status</th><th>Verdict</th></tr>{rows}</table></div>"
    return page("Seed campaign", b)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--seeds", required=True, help="one post list or several, comma-separated")
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--site", type=Path, required=True)
    ap.add_argument("--mirror", type=Path)
    a = ap.parse_args()
    built = dt.datetime.now(dt.UTC).strftime("%Y-%m-%d %H:%M:%S UTC")
    posts = [Post(s, a.out, a.mirror) for s in seeds(a.seeds)]
    (a.site / "t").mkdir(parents=True, exist_ok=True)
    for p in posts:
        tmp = a.site / "t" / f".{p.id}.html"
        tmp.write_text(tweet_page(p, built), "utf-8")
        os.replace(tmp, a.site / "t" / f"{p.id}.html")
    tmp = a.site / ".index.html"
    tmp.write_text(index_page(posts, built), "utf-8")
    os.replace(tmp, a.site / "index.html")
    full = sum(1 for p in posts if p.state()[0] == "full")
    print(f"{len(posts)} tweet pages and the index in {a.site}; {full} full backtests")


if __name__ == "__main__":
    main()

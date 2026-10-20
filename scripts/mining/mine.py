#!/usr/bin/env python3
"""Campaign 2's mining (the owner, Oct 19, 2026): the strategy accounts of
docs/design/fixtures/strategy-accounts.toml searched by date window through
xAI's x_search (PL-D9; no X API token), every response kept under
data/mining/raw/, its cost summed against a hard cap before each request.

    python3 scripts/mining/mine.py --cap-usd 15 [--days 45] [--dry-run]

Read-only on X; nothing is posted, liked or followed. The posts found are
written to data/mining/posts.jsonl (one JSON object per post, the fields the
prompt asks for plus `handle` and `window`)."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
import time
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "data" / "mining"
RAW = OUT / "raw"
SPEND = OUT / "spend.jsonl"
POSTS = OUT / "posts.jsonl"
MODEL = "grok-4.20-non-reasoning"
TODAY = dt.date(2026, 10, 19)

PROMPT = (
    "Search X for original posts by @{handle} from {frm} to {to} (UTC, inclusive) that state a trading idea a quant could test: "
    "a strategy or rule (entry, exit, stop), a backtest or its statistics (win rate, profit factor, CAGR, Sharpe, drawdown, trades), "
    "an indicator signal (moving averages, RSI, ATR, Bollinger, breadth, COT), a seasonality or calendar effect, or a chart pattern "
    "with explicit levels on a named market. Skip replies to other accounts, reposts, promotions of courses, products or communities, "
    "and posts with no market claim. Return up to {n} such posts, most rule-like first, as JSON lines with exactly the keys id (the "
    "numeric status id as a string), url, created_at (ISO-8601 UTC), text (verbatim, no paraphrase), urls (expanded links), "
    "quoted_id (or null), reply_to_id (or null). Output JSON lines only. Omit any post you cannot see verbatim."
)


def spent() -> float:
    if not SPEND.exists():
        return 0.0
    return sum(float(json.loads(line).get("cost_usd") or 0) for line in SPEND.read_text().splitlines() if line.strip())


def windows(days: int) -> list[tuple[dt.date, dt.date]]:
    """One window over the last `days` days (the owner: the smallest windows that yield posts)."""
    return [(TODAY - dt.timedelta(days=days), TODAY)]


def ask(key: str, handle: str, frm: dt.date, to: dt.date, n: int) -> dict:
    body = {
        "model": MODEL,
        "input": PROMPT.format(handle=handle, frm=frm.isoformat(), to=to.isoformat(), n=n),
        "tools": [{"type": "x_search", "allowed_x_handles": [handle], "from_date": frm.isoformat(), "to_date": to.isoformat()}],
        "max_tool_calls": 6,
    }
    req = urllib.request.Request(
        "https://api.x.ai/v1/responses",
        data=json.dumps(body).encode(),
        headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.loads(r.read())


def lines_of(resp: dict) -> list[dict]:
    out = []
    for o in resp.get("output") or []:
        if o.get("type") != "message":
            continue
        for c in o.get("content") or []:
            for line in str(c.get("text") or "").splitlines():
                line = line.strip().strip("`")
                if line.startswith("json"):
                    line = line[4:].strip()
                if not line.startswith("{"):
                    continue
                try:
                    p = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if p.get("id"):
                    p["id"] = str(p["id"])
                    out.append(p)
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cap-usd", type=float, default=15.0)
    ap.add_argument("--days", type=int, default=45)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    RAW.mkdir(parents=True, exist_ok=True)
    key = (ROOT / "SECRETS" / "xai_api_key").read_text().strip()
    accounts = tomllib.loads((ROOT / "docs/design/fixtures/strategy-accounts.toml").read_text())["account"]
    done = {tuple(json.loads(line)["window"]) for line in SPEND.read_text().splitlines()} if SPEND.exists() else set()
    seen = {json.loads(line)["id"] for line in POSTS.read_text().splitlines()} if POSTS.exists() else set()
    for acc in accounts:
        for frm, to in windows(a.days):
            w = (acc["handle"], frm.isoformat(), to.isoformat())
            if w in done:
                continue
            # The guard: no request once the spend plus the dearest response so far reaches the cap.
            if spent() + 0.6 > a.cap_usd:
                print(f"cap: ${spent():.2f} of ${a.cap_usd:.2f}; stopped before @{acc['handle']} {frm}..{to}", flush=True)
                return 0
            if a.dry_run:
                print("would ask", w)
                continue
            t0 = time.time()
            try:
                resp = ask(key, acc["handle"], frm, to, 25)
            except Exception as e:  # noqa: BLE001 - one window's failure is logged, the rest go on
                print(f"@{acc['handle']} {frm}..{to}: {type(e).__name__}: {e}", flush=True)
                continue
            name = f"{acc['handle']}-{frm}-{to}.json"
            (RAW / name).write_text(json.dumps(resp))
            u = resp.get("usage") or {}
            cost = float(u.get("cost_in_usd_ticks") or 0) * 1e-10
            posts = lines_of(resp)
            with SPEND.open("a") as f:
                f.write(json.dumps({"service": "xai", "what": "campaign-2 mining", "window": list(w), "cost_usd": cost, "posts": len(posts),
                                    "x_posts_fetched": (u.get("server_side_tool_usage_details") or {}).get("x_posts_fetched"), "raw": name,
                                    "r": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}) + "\n")
            new = 0
            with POSTS.open("a") as f:
                for p in posts:
                    if p["id"] in seen:
                        continue
                    seen.add(p["id"])
                    new += 1
                    f.write(json.dumps({**p, "handle": acc["handle"], "window": [frm.isoformat(), to.isoformat()]}, ensure_ascii=False) + "\n")
            print(f"@{acc['handle']} {frm}..{to}: {len(posts)} posts ({new} new), ${cost:.3f}, {time.time() - t0:.0f} s; spent ${spent():.2f}", flush=True)
    print(f"done; spent ${spent():.2f}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())

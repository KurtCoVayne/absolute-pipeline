#!/usr/bin/env python3
"""Campaign 2's local pre-filter (no model): the mined posts of
data/mining/posts.jsonl scored for strategy content, the best per account
kept, each kept post's verified text fetched from the public mirror into
data/seed/<id>.json, and docs/design/fixtures/campaign-2-posts.tsv written.

    python3 scripts/mining/filter.py [--keep 8]

Dropped: replies to other accounts, reposts, posts promoting courses,
products or communities, one-liners, posts already in the seed list."""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
import urllib.request
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
POSTS = ROOT / "data" / "mining" / "posts.jsonl"
MIRROR = ROOT / "data" / "seed"
SEED_TSV = ROOT / "docs" / "design" / "fixtures" / "seed-posts.tsv"
OUT_TSV = ROOT / "docs" / "design" / "fixtures" / "campaign-2-posts.tsv"
UA = "absolute-pipeline/0.1 (research, read-only)"

RULE = [
    r"\bentry\b", r"\benter\b", r"\bexit\b", r"\bstop\b", r"\bstop[- ]loss\b", r"\bif\b.*\bthen\b", r"\bwhen\b", r"\bcross(es|ed|over)?\b",
    r"\babove\b", r"\bbelow\b", r"\brsi\b", r"\bsma\b", r"\bema\b", r"\bmoving average\b", r"\b\d+[- ]?(day|week|bar|period)\b", r"\batr\b",
    r"\bbollinger\b", r"\bbreak(out|s)?\b", r"\bmean reversion\b", r"\bseasonal", r"\bmonday|tuesday|wednesday|thursday|friday\b",
    r"\bturn of (the )?month\b", r"\bbacktest", r"\bwin rate\b", r"\bprofit factor\b", r"\bsharpe\b", r"\bcagr\b", r"\bdrawdown\b",
    r"\btrades?\b", r"\bsince (19|20)\d\d\b", r"\bequity curve\b", r"\bibs\b", r"\bclose[sd]?\b", r"\bopen\b", r"\bpattern\b",
    r"\bsupport\b", r"\bresistance\b", r"\btarget\b", r"\bsignal\b", r"\bstrategy\b", r"\brules?\b", r"\bhold(ing)?\b",
]
NUM = [r"\d+(\.\d+)?\s?%", r"\b\d+(\.\d+)?\b"]
PROMO = re.compile(r"(whop\.com|discord|gumroad|patreon|course|webinar|join (my|our)|sign up|subscribe|link in bio|use code|% off|free trial|newsletter|substack\.com)", re.I)


def score(text: str) -> float:
    t = text.lower()
    s = sum(1.0 for p in RULE if re.search(p, t))
    s += min(4.0, 0.5 * sum(len(re.findall(p, t)) for p in NUM[:1]))
    s += min(2.0, 0.1 * len(re.findall(NUM[1], t)))
    return s


def created(post_id: str) -> str:
    ms = (int(post_id) >> 22) + 1288834974657
    return datetime.fromtimestamp(ms / 1000, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def mirror(handle: str, post_id: str) -> dict | None:
    path = MIRROR / f"{post_id}.json"
    if path.exists():
        return json.loads(path.read_text())
    req = urllib.request.Request(f"https://api.fxtwitter.com/{handle}/status/{post_id}", headers={"User-Agent": UA})
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            d = json.loads(r.read())
    except Exception:  # noqa: BLE001 - a post the mirror cannot serve is skipped
        return None
    if d.get("code") != 200 or str(d.get("tweet", {}).get("id")) != post_id:
        return None
    d["seed"] = {"handle": handle, "post_id": post_id, "created_utc": created(post_id), "url": f"https://x.com/{handle}/status/{post_id}", "fetched_by": "fxtwitter", "campaign": 2}
    path.write_text(json.dumps(d))
    time.sleep(0.4)
    return d


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep", type=int, default=8)
    a = ap.parse_args()
    seed_ids = {line.split("\t")[2] for line in SEED_TSV.read_text().splitlines() if line and not line.startswith(("#", "created_utc"))}
    by: dict[str, list[tuple[float, dict]]] = defaultdict(list)
    dropped: dict[str, int] = defaultdict(int)
    for line in POSTS.read_text().splitlines():
        p = json.loads(line)
        text = str(p.get("text") or "")
        if p["id"] in seed_ids:
            dropped["already a seed post"] += 1
        elif text.startswith("RT @"):
            dropped["repost"] += 1
        elif PROMO.search(text):
            dropped["promotion"] += 1
        elif len(text.strip()) < 60:
            dropped["one-liner"] += 1
        else:
            by[p["handle"]].append((score(text), p))
    rows = []
    for handle, items in sorted(by.items()):
        items.sort(key=lambda x: -x[0])
        kept = 0
        for s, p in items:
            if kept >= a.keep or s < 3:
                dropped["low score or over the per-account keep"] += 1
                continue
            m = mirror(handle, p["id"])
            if m is None:
                dropped["the mirror cannot serve it"] += 1
                continue
            t = m["tweet"]
            replying = str(t.get("replying_to") or "")
            if replying and replying.lower() != handle.lower():
                dropped["reply to another account"] += 1
                continue
            rows.append((created(p["id"]), handle, p["id"], f"https://x.com/{handle}/status/{p['id']}", s))
            kept += 1
        print(f"@{handle}: {len(items)} candidates, {kept} kept", flush=True)
    rows.sort()
    head = (
        "# Campaign 2 (the owner, Oct 19, 2026): posts of the strategy accounts (strategy-accounts.toml) from the last 45 days, mined through\n"
        "# xAI's x_search, pre-filtered locally by scripts/mining/filter.py, verified through the public mirror. created_utc from the snowflake.\n"
        "created_utc\thandle\tpost_id\turl\n"
    )
    OUT_TSV.write_text(head + "".join(f"{c}\t{h}\t{i}\t{u}\n" for c, h, i, u, _ in rows))
    print(f"{len(rows)} posts kept; dropped {dict(dropped)}; wrote {OUT_TSV.relative_to(ROOT)}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())

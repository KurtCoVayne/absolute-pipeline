"""A post list's verified copies from the public mirror (no X API, no xAI).

Each post of a list (`created_utc handle post_id url`, the fixtures' form) is
fetched from api.fxtwitter.com into MIRROR/<post_id>.json unless it is there
already, with the `seed` block `filter.py` writes; a post the mirror cannot
serve is reported and skipped. `pipeline-run fetch --mirror-only` then makes
the queue's documents from these copies.

    python3 -I scripts/mining/stash.py docs/design/fixtures/campaign-3-posts.tsv /srv/absolute/data/seed 3
"""

from __future__ import annotations

import json
import sys
import time
import urllib.request
from pathlib import Path

UA = "absolute-pipeline/0.1 (research; read-only)"


def rows(path: Path) -> list[dict[str, str]]:
    out = []
    header: list[str] | None = None
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        cells = line.split("\t")
        if header is None:
            header = cells
            continue
        out.append(dict(zip(header, cells)))
    return out


def main() -> int:
    posts, mirror, campaign = Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3])
    mirror.mkdir(parents=True, exist_ok=True)
    kept = missing = 0
    for p in rows(posts):
        path = mirror / f"{p['post_id']}.json"
        if path.exists():
            kept += 1
            continue
        req = urllib.request.Request(f"https://api.fxtwitter.com/{p['handle']}/status/{p['post_id']}", headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=20) as r:
                d = json.loads(r.read())
        except Exception as e:  # noqa: BLE001 - a post the mirror cannot serve is reported
            print(f"{p['post_id']} @{p['handle']}: the mirror cannot serve it ({type(e).__name__})")
            missing += 1
            continue
        if d.get("code") != 200 or str(d.get("tweet", {}).get("id")) != p["post_id"]:
            print(f"{p['post_id']} @{p['handle']}: the mirror answered {d.get('code')}")
            missing += 1
            continue
        d["seed"] = {"handle": p["handle"], "post_id": p["post_id"], "created_utc": p["created_utc"], "url": p["url"], "fetched_by": "fxtwitter", "campaign": campaign}
        path.write_text(json.dumps(d))
        kept += 1
        time.sleep(0.4)
    print(f"{kept} copies in {mirror}; {missing} the mirror cannot serve")
    return 0


if __name__ == "__main__":
    sys.exit(main())

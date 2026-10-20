"""The extra Databento roots through d20-research's cost-gated writer (see dbn-pull-extra.sh).
stdin-free: reads jobs.txt beside it ("DATASET ROOT START" per line); argv: CACHE END."""
from __future__ import annotations

import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from d20research.data import ingest, layout

cache, end = Path(sys.argv[1]), sys.argv[2]
layout.CACHE_ROOT = cache  # bars_dir/definition_path read it at call time


def one(line: str) -> str:
    dataset, root, start = line.split()
    try:
        n_def = ingest.ingest_definitions(root, dataset, start, end)
        rep = ingest.ingest_bars(root, dataset, start, end)
        return f"{dataset} {root}: {n_def} contracts, {rep.days_written} days written, {rep.days_skipped_existing} kept, {rep.rows} rows"
    except ingest.CostGate as e:
        return f"{dataset} {root}: REFUSED by the cost gate: {e}"
    except Exception as e:  # noqa: BLE001 - one root's failure never stops the rest
        return f"{dataset} {root}: FAILED {type(e).__name__}: {e}"


jobs = [ln for ln in (Path(__file__).parent / "jobs.txt").read_text().splitlines() if ln.strip()]
with ThreadPoolExecutor(max_workers=4) as ex:
    for r in ex.map(one, jobs):
        print(r, flush=True)

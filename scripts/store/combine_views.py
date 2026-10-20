#!/usr/bin/env python3
"""A root's history and its live/ extension as one Databento view, so one ingest rolls and
back-adjusts the whole span (an ingest leaves its own last contract unadjusted; a separate live
ingest would not join the history at its first roll). The day files are linked from the
history's outright-filtered files (or the vendor's) and from live/; the definitions are the two
files' union, an instrument defined in both kept once (live's row).
Usage: combine_views.py OUT_DIR ROOT..."""
import sys
from pathlib import Path

import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq

VENDOR = Path("/srv/absolute/data/vendor")
HIST, LIVE = VENDOR / "dbn/GLBX.MDP3", VENDOR / "dbn-live/GLBX.MDP3"
FILTERED = Path("/srv/absolute/full/outrights/GLBX.MDP3/ohlcv-1m")

out = Path(sys.argv[1])
for root in sys.argv[2:]:
    hist = FILTERED / f"{root}.FUT" if (FILTERED / f"{root}.FUT.done").exists() else HIST / "ohlcv-1m" / f"{root}.FUT"
    days = out / root / "ohlcv-1m" / f"{root}.FUT"
    days.mkdir(parents=True, exist_ok=True)
    for f in days.iterdir():
        f.unlink()
    names = {}
    for d in (hist, LIVE / "ohlcv-1m" / f"{root}.FUT"):
        for f in sorted(d.glob("*.parquet")):
            names[f.name] = f
    for name, f in names.items():
        (days / name).symlink_to(f)
    defs = [pq.read_table(p) for p in (HIST / "definition" / f"{root}.parquet", LIVE / "definition" / f"{root}.parquet") if p.exists()]
    t = pa.concat_tables(defs, promote_options="permissive") if len(defs) > 1 else defs[0]
    ids = t.column("instrument_id").to_pylist()
    last = {i: k for k, i in enumerate(ids)}
    t = t.take(pa.array(sorted(last.values())))
    (out / root / "definition").mkdir(parents=True, exist_ok=True)
    pq.write_table(t, out / root / "definition" / f"{root}.parquet")
    print(root, len(names), "day files,", t.num_rows, "definitions,", "history from", hist.parent.parent.name, hist.name)

#!/usr/bin/env python3
"""A root's Databento day files reduced to the bars of the instruments its definition file
defines (the outright contracts: spreads and undefined instruments are what the adapter's
`--undefined drop` drops anyway), so a large root's ingest holds a fraction of the rows in
memory. The kept rows are byte-identical in content; the adapter's report counts fewer drops.
Usage: prefilter_outrights.py DATASET_DIR ROOT OUT_DIR"""
import sys
from pathlib import Path

import pyarrow.compute as pc
import pyarrow.parquet as pq

src, root, out = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
ids = pq.read_table(src / "definition" / f"{root}.parquet", columns=["instrument_id"]).column("instrument_id")
dst = out / "ohlcv-1m" / f"{root}.FUT"
dst.mkdir(parents=True, exist_ok=True)
(out / "definition").mkdir(parents=True, exist_ok=True)
link = out / "definition" / f"{root}.parquet"
if not link.exists():
    link.symlink_to(src / "definition" / f"{root}.parquet")
n_in = n_out = 0
for f in sorted((src / "ohlcv-1m" / f"{root}.FUT").glob("*.parquet")):
    t = pq.read_table(f)
    k = t.filter(pc.is_in(t.column("instrument_id"), value_set=ids))
    n_in += t.num_rows
    n_out += k.num_rows
    pq.write_table(k, dst / f.name, compression="zstd")
print(f"{root}: {n_in} rows in, {n_out} kept ({n_out / max(n_in, 1):.1%})")

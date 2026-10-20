#!/usr/bin/env python3
"""A root's Databento day files reduced to the bars of its outright contracts, recognised by
the raw symbol (root, month code, one- or two-digit year: `RBZ8`, `CLF26`), so a large root's
ingest holds a fraction of the rows in memory. Spreads, butterflies and inter-commodity pairs
are what the adapter drops anyway; an outright the definition file lacks is kept, for the
adapter's synthesized definition (E-104). The kept rows are byte-identical in content; the
adapter's report counts fewer drops.
Usage: prefilter_outrights.py DATASET_DIR ROOT OUT_DIR"""
import re
import sys
from pathlib import Path

import pyarrow.compute as pc
import pyarrow.parquet as pq

src, root, out = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
outright = f"^{re.escape(root)}[FGHJKMNQUVXZ][0-9]{{1,2}}$"
dst = out / "ohlcv-1m" / f"{root}.FUT"
dst.mkdir(parents=True, exist_ok=True)
(out / "definition").mkdir(parents=True, exist_ok=True)
link = out / "definition" / f"{root}.parquet"
if not link.exists():
    link.symlink_to(src / "definition" / f"{root}.parquet")
n_in = n_out = 0
for f in sorted((src / "ohlcv-1m" / f"{root}.FUT").glob("*.parquet")):
    t = pq.read_table(f)
    k = t.filter(pc.match_substring_regex(t.column("symbol"), outright))
    n_in += t.num_rows
    n_out += k.num_rows
    pq.write_table(k, dst / f.name, compression="zstd")
(out / "ohlcv-1m" / f"{root}.FUT.done").write_text(f"{n_in} {n_out}\n")
print(f"{root}: {n_in} rows in, {n_out} kept ({n_out / max(n_in, 1):.1%})")

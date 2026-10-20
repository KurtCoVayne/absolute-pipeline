#!/usr/bin/env python3
"""The Norgate lake's continuous futures (daily, from 1978) to one bundle_v2 directory of the
future class under the source `norgate` (docs/iterations/01-data.md). KIND `n` (the default)
takes the back-adjusted series (`&<ROOT>_CCB`); KIND `u` the unadjusted ones (`&<ROOT>`, the
front contract's own prices, rolled), whose levels a spread's rule can read.

Each series becomes a continuous instrument with the ticker `<ROOT>_N` or `<ROOT>_U` and the
vendor key `n:<ROOT>` or `u:<ROOT>` (never colliding with Databento's `ES` or TradeStation's
`ES.ts`): one bar per trade date labelled at 21:00 UTC (the
vendor gives dates only; a session's close is approximated so, and its open 23 hours before), a
session row per bar, and point_value and tick per day from the root's latest contract in
registry/futures_reference (its point value and minimum move). The bundle's resolution is 1d.
Usage: norgate_futures_to_bundle.py LAKE OUT_DIR [KIND]"""
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq

lake, out = Path(sys.argv[1]), Path(sys.argv[2])
kind = sys.argv[3] if len(sys.argv) > 3 else "n"
suffix, form = {"n": ("_CCB", "&<ROOT>_CCB"), "u": ("", "&<ROOT>")}[kind]
sm = pq.read_table(lake / "registry/security_master/data.parquet").to_pandas()
sm["valid_from"] = pd.to_datetime(sm["valid_from"])
cur = sm.sort_values("valid_from").groupby("asset_id").tail(1)
ccb = cur[cur.ticker.str.match(rf"^&[A-Z0-9]+{suffix}$", na=False)].copy()
ccb["root"] = ccb.ticker.str.slice(1).str.removesuffix(suffix)
fr = pq.read_table(lake / "registry/futures_reference/data.parquet").to_pandas()
fr["root"] = fr.source_symbol.str.split("-").str[0]
fr = fr.sort_values("last_quoted_date")
pv = fr.groupby("root").tail(1).set_index("root")[["point_value", "tick_size"]]
ccb = ccb.join(pv, on="root")
missing_pv = ccb[ccb.point_value.isna()].ticker.tolist()
ccb = ccb[ccb.point_value.notna()]
ids = set(ccb.asset_id.tolist())
bars = ds.dataset(lake / "curated/bars_by_date", format="parquet", partitioning="hive")
cols = ["asset_id", "date", "open", "high", "low", "close", "volume"]
t = bars.to_table(columns=cols, filter=ds.field("asset_id").isin(list(ids))).to_pandas()
t["date"] = pd.to_datetime(t["date"])
t = t.sort_values(["asset_id", "date"]).drop_duplicates(["asset_id", "date"], keep="last")
t["T"] = t["date"] + pd.Timedelta(hours=21)
t = t[t[["open", "high", "low", "close"]].notna().all(axis=1)]
t["volume"] = t["volume"].fillna(0.0).astype(float)
key = {a: f"{kind}:{r}" for a, r in zip(ccb.asset_id, ccb.root)}
t["A"] = t["asset_id"].map(key)
d = out / "futures"
rel = []


def part(name: str, frame: pd.DataFrame, cols: list[str], kinds: list) -> None:
    arrays = []
    for c, k in zip(cols, kinds):
        v = frame[c]
        arrays.append(pa.array(v.values.astype("datetime64[ns]"), pa.timestamp("ns")) if k == "t" else pa.array(v.values, pa.string() if k == "s" else pa.float64()))
    tbl = pa.Table.from_arrays(arrays, names=cols)
    p = f"log/{name}/all.parquet"
    (d / p).parent.mkdir(parents=True, exist_ok=True)
    pq.write_table(tbl, d / p, compression="zstd")
    rel.append({"name": name, "signature": name, "resolution": "1d", "availability": "bar_close", "rows": tbl.num_rows, "partitions": [p]})


for name in ("open", "high", "low", "close", "volume"):
    part(name, t.rename(columns={name: "V"}), ["A", "T", "V"], ["s", "t", "f"])
sess = t[["A", "T"]].copy()
sess["Open"] = sess["T"] - pd.Timedelta(hours=23)
part("session", sess.rename(columns={"T": "S"}), ["A", "S", "Open"], ["s", "t", "t"])
pvr = t[["A", "date"]].copy()
pvr["V"] = t["asset_id"].map(dict(zip(ccb.asset_id, ccb.point_value))).astype(float)
part("point_value", pvr.rename(columns={"date": "T"}), ["A", "T", "V"], ["s", "t", "f"])
tkr = t[["A", "date"]].copy()
tkr["V"] = t["asset_id"].map(dict(zip(ccb.asset_id, ccb.tick_size))).astype(float)
tkr = tkr[tkr["V"].notna()]
part("tick", tkr.rename(columns={"date": "T"}), ["A", "T", "V"], ["s", "t", "f"])
first = t.groupby("A")["T"].min()
sec = ccb.assign(id=ccb.asset_id.map(key))
sec = sec[sec.id.isin(first.index)]
pq.write_table(pa.table({"id": sec.id.tolist(), "ticker": (sec.root + "_" + kind.upper()).tolist(),
                         "from": pa.array(first.loc[sec.id].values.astype("datetime64[ns]"), pa.timestamp("ns")),
                         "to": pa.array([None] * len(sec), pa.timestamp("ns")), "multiplier": sec.point_value.astype(float).tolist(),
                         "asset_class": ["future"] * len(sec)}), d / "securities.parquet")
(d / "manifest.json").write_text(json.dumps({"format": 2, "name": "future", "version": f"norgate-{kind}-2026-10-19", "as_of": "2026-10-19", "processing_delay_seconds": 0,
                                             "source": "norgate", "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "relations": rel,
                                             "tests": {"converted_from": f"norgate lake {form} series", "series": len(sec), "bars": len(t)}}, indent=1))
report = {"series": len(sec), "bars": len(t), "first": str(t["T"].min()), "last": str(t["T"].max()), "missing_point_value": missing_pv,
          "roots": {r: [str(first.get(f"{kind}:{r}")), float(p)] for r, p in zip(sec.root, sec.point_value)}}
(d / "report.json").write_text(json.dumps(report, indent=1))
print(json.dumps({k: report[k] for k in ("series", "bars", "first", "last", "missing_point_value")}))

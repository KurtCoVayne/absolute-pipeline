#!/usr/bin/env python3
"""TradeStation continuous 1-minute bars (~/d20-research/data/tradestation/cache/<TICKER>.parquet,
d20-research's parse of raw/<TICKER>.txt: `ts` the bar's START in naive CT wall clock, volume =
up + down; sparse, additively back-adjusted) to bundle_v2 directories of
the future class, one per root, under the source `tradestation` (docs/iterations/01-data.md).

Each root becomes one continuous instrument whose ticker is `<CME root>.ts` (never colliding
with Databento's `ES.v.0`): bars labelled at their minute's close in UTC, one session per CME
trading day (the Globex day rolls at 17:00 CT: a bar ending after 17:00 CT belongs to the next
day's session; labelled at its last bar's close, opening at its first bar's start), point_value
per session day from d20-research's contract specs. A gap rule would merge the ES family's
pre-2012 sessions, whose 30-minute break is shorter than any sensible gap. An hour the CT fall-back makes ambiguous is dropped (overnight minutes once a year).
Usage: ts_to_bundle.py CACHE_DIR SPECS_DIR OUT_DIR [TICKER ...]"""
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq
import yaml

TS_TO_ROOT = {
    "EC": "6E", "FV": "ZF", "TU": "ZT", "US": "ZB", "TY": "ZN", "ES": "ES", "NQ": "NQ", "RTY": "RTY", "NK": "NKD",
    "CL": "CL", "NG": "NG", "RB": "RB", "HO": "HO", "C": "ZC", "S": "ZS", "BO": "ZL", "W": "ZW", "SM": "ZM",
    "LC": "LE", "LH": "HE", "FC": "GF", "GC": "GC", "HG": "HG", "SI": "SI", "PL": "PL", "JY": "6J", "MP1": "6M",
    "BTC": "BTC", "ETH": "ETH", "SB": "SB", "CC": "CC", "CT": "CT", "KC": "KC",
    "AD": "6A", "SF": "6S", "YM": "YM", "EMD": "EMD", "DX": "DX", "BZ": "BZ", "KW": "KE", "O": "ZO", "RR": "ZR",
    "LBR": "LBR", "OJ": "OJ", "PA": "PA", "SR3": "SR3",
}
EXCLUDED = {"RA": "unidentified contract (d20-research's R25 registry excludes it)"}
NS = "datetime64[ns, UTC]"


def specs(d: Path) -> dict[str, float]:
    bpv = {k: float(v["bpv"]) for k, v in yaml.safe_load((d / "contract_specs.yaml").read_text())["specs"].items()}
    for k, v in yaml.safe_load((d / "tradestation_specs.yaml").read_text())["new_roots"].items():
        if "bpv" in v and k in TS_TO_ROOT:
            bpv[TS_TO_ROOT[k]] = float(v["bpv"])
    return bpv


def write(path: Path, table: pa.Table) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pq.write_table(table, path, compression="zstd")


def convert(raw: Path, ticker: str, root: str, bpv: float, out: Path) -> dict:
    df = pq.read_table(raw).to_pandas()
    ct_end = pd.to_datetime(df["ts"]) + pd.Timedelta(minutes=1)
    end = ct_end.dt.tz_localize("America/Chicago", ambiguous="NaT", nonexistent="shift_forward").dt.tz_convert("UTC")
    keep = end.notna()
    df, end = df[keep], end[keep]
    df = pd.DataFrame({"t": end.values, "open": df["open"].astype(float).values, "high": df["high"].astype(float).values,
                       "low": df["low"].astype(float).values, "close": df["close"].astype(float).values,
                       "volume": df["volume"].astype(float).values})
    df = df.sort_values("t").drop_duplicates("t", keep="last").reset_index(drop=True)
    df["t"] = pd.to_datetime(df["t"], utc=True)
    sym = f"{root}.ts"
    ident = f"ts:{root}"
    # Sessions: the CME trading day, which rolls at 17:00 CT.
    start = df["t"] - pd.Timedelta(minutes=1)
    ct = df["t"].dt.tz_convert("America/Chicago")
    tday = (ct + pd.Timedelta(hours=7)).dt.date
    sess = df.assign(start=start, sid=tday).groupby("sid").agg(S=("t", "max"), Open=("start", "min"))
    days = sess["S"].dt.floor("D").drop_duplicates()
    d = out / root
    rel = []
    def part(name: str, cols: dict) -> None:
        t = pa.table(cols)
        p = f"log/{name}/all.parquet"
        write(d / p, t)
        rel.append({"name": name, "signature": name, "resolution": "1m", "availability": "bar_close", "rows": t.num_rows, "partitions": [p]})
    ids = pa.array([ident] * len(df), pa.string())
    tcol = pa.array(df["t"].values.astype("datetime64[ns]"), pa.timestamp("ns"))
    for name in ("open", "high", "low", "close", "volume"):
        part(name, {"A": ids, "T": tcol, "V": pa.array(df[name].values, pa.float64())})
    part("session", {"A": pa.array([ident] * len(sess), pa.string()), "S": pa.array(sess["S"].values.astype("datetime64[ns]"), pa.timestamp("ns")),
                     "Open": pa.array(sess["Open"].values.astype("datetime64[ns]"), pa.timestamp("ns"))})
    part("point_value", {"A": pa.array([ident] * len(days), pa.string()), "T": pa.array(days.values.astype("datetime64[ns]"), pa.timestamp("ns")),
                         "V": pa.array([bpv] * len(days), pa.float64())})
    write(d / "securities.parquet", pa.table({"id": [ident], "ticker": [sym], "from": pa.array([int(df["t"].iloc[0].value)], pa.timestamp("ns")),
                                              "to": pa.array([None], pa.timestamp("ns")), "multiplier": [bpv], "asset_class": ["future"]}))
    manifest = {"format": 2, "name": "future", "version": f"tradestation-{root}-2026-10-19", "as_of": "2026-10-19", "processing_delay_seconds": 0,
                "source": "tradestation", "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), "relations": rel,
                "tests": {"converted_from": f"tradestation raw {ticker}.txt", "rows": len(df), "sessions": len(sess)}}
    (d / "manifest.json").write_text(json.dumps(manifest, indent=1))
    return {"ticker": ticker, "root": root, "symbol": sym, "bars": len(df), "sessions": len(sess), "first": str(df["t"].iloc[0]), "last": str(df["t"].iloc[-1]), "bpv": bpv}


def main() -> None:
    raw, spec_dir, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    want = sys.argv[4:] or sorted(p.stem for p in raw.glob("*.parquet"))
    bpv = specs(spec_dir)
    report = []
    for tk in want:
        if tk in EXCLUDED:
            report.append({"ticker": tk, "excluded": EXCLUDED[tk]})
            continue
        root = TS_TO_ROOT.get(tk)
        if root is None or root not in bpv:
            report.append({"ticker": tk, "excluded": "no root or point value"})
            continue
        r = convert(raw / f"{tk}.parquet", tk, root, bpv[root], out)
        print(json.dumps(r), flush=True)
        report.append(r)
    (out / "report.json").write_text(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()

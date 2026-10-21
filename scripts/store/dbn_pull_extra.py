"""The extra Databento roots through d20-research's cost-gated writer (see dbn-pull-extra.sh).
Reads jobs.txt beside it ("DATASET ROOT START" per line); argv: CACHE END.

The writer's own chunks (a quarter of definitions, a year of bars per request) time out on Databento's
gateway (504) for spread-heavy roots; here the same gate, columns, dedupe and atomic per-day files, with
one month per request and server errors retried.
"""
from __future__ import annotations

import os
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import databento as db
import pandas as pd

from d20research.data import ingest, layout

cache, end = Path(sys.argv[1]), sys.argv[2]
layout.CACHE_ROOT = cache  # bars_dir/definition_path read it at call time


def months(start: str, stop: str) -> list[tuple[str, str]]:
    out = []
    for m in pd.period_range(start=start, end=pd.Timestamp(stop) - pd.Timedelta(days=1), freq="M"):
        s = max(pd.Timestamp(start), m.start_time).strftime("%Y-%m-%d")
        e = min(pd.Timestamp(stop), m.end_time + pd.Timedelta(days=1)).strftime("%Y-%m-%d")
        if s < e:
            out.append((s, e))
    return out


def fetch(client: db.Historical, dataset: str, root: str, schema: str, s: str, e: str) -> pd.DataFrame:
    ingest._gate(client, dataset=dataset, symbols=[f"{root}.FUT"], schema=schema, start=s, end=e, max_cost_usd=0.01)
    for attempt in range(6):
        try:
            return client.timeseries.get_range(dataset=dataset, symbols=[f"{root}.FUT"], schema=schema,
                                               start=s, end=e, stype_in="parent").to_df(price_type="float")
        except db.BentoClientError as x:
            if "symbology_invalid_request" in str(x):
                return pd.DataFrame()  # not listed in this month
            if attempt == 5:
                raise
        except db.BentoError:  # a server error or a stream cut short: retry
            if attempt == 5:
                raise
        time.sleep(15 * (attempt + 1))
    return pd.DataFrame()


def definitions(client: db.Historical, root: str, dataset: str, start: str) -> int:
    frames = []
    for s, e in months(start, end):
        df = fetch(client, dataset, root, "definition", s, e)
        if df.empty:
            continue
        df = df.reset_index()
        if "raw_symbol" not in df.columns and "symbol" in df.columns:
            df["raw_symbol"] = df["symbol"]
        df["root"] = root
        frames.append(df[[c for c in ingest.DEF_COLUMNS if c in df.columns]].drop_duplicates(subset=["raw_symbol"], keep="last"))
    if not frames:
        return 0
    out = pd.concat(frames, ignore_index=True).sort_values("expiration").drop_duplicates(subset=["raw_symbol"], keep="last")
    path = layout.definition_path(dataset, root)
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        out = pd.concat([pd.read_parquet(path), out], ignore_index=True).drop_duplicates(subset=["raw_symbol"], keep="last")
    out = out.sort_values("expiration").reset_index(drop=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, suffix=".tmp")
    os.close(fd)
    out.to_parquet(tmp, index=False)
    os.replace(tmp, path)
    return len(out)


def bars(client: db.Historical, root: str, dataset: str, start: str) -> tuple[int, int, int]:
    out_dir = layout.bars_dir(dataset, root)
    out_dir.mkdir(parents=True, exist_ok=True)
    written = kept = rows = 0
    have = {f.stem[:7] for f in out_dir.glob("*.parquet")}
    for s, e in months(start, end):
        # A rerun skips a month already on disk (any day of it), except the last one, which may be partial.
        if s[:7] in have and s[:7] != max(have):
            continue
        df = fetch(client, dataset, root, "ohlcv-1m", s, e)
        if df.empty:
            continue
        df = df.reset_index()
        df = df[[c for c in ingest.BAR_COLUMNS if c in df.columns]].drop_duplicates(subset=["instrument_id", "ts_event"], keep="first")
        df["volume"] = df["volume"].astype("uint64")
        df["instrument_id"] = df["instrument_id"].astype("uint32")
        df["symbol"] = df["symbol"].astype(str)
        for day, day_df in df.groupby(layout.session_day(df["ts_event"], dataset)):
            path = out_dir / f"{day}.parquet"
            if path.exists():
                kept += 1
                continue
            day_df = day_df.sort_values(["ts_event", "instrument_id"]).reset_index(drop=True)
            fd, tmp = tempfile.mkstemp(dir=out_dir, suffix=".tmp")
            os.close(fd)
            day_df.to_parquet(tmp, index=False)
            os.replace(tmp, path)
            written += 1
            rows += len(day_df)
    return written, kept, rows


def one(line: str) -> str:
    dataset, root, start = line.split()
    client = db.Historical()  # DATABENTO_API_KEY from env
    try:
        # SKIP_DEFS=1: bars only, while Databento's definition requests time out (status "partially
        # degraded", Oct 21); the definitions follow in a later run.
        n_def = 0 if os.environ.get("SKIP_DEFS") == "1" else definitions(client, root, dataset, start)
        w, k, r = bars(client, root, dataset, start)
        return f"{dataset} {root}: {n_def} contracts, {w} days written, {k} kept, {r} rows"
    except ingest.CostGate as e:
        return f"{dataset} {root}: REFUSED by the cost gate: {e}"
    except Exception as e:  # noqa: BLE001 - one root's failure never stops the rest
        return f"{dataset} {root}: FAILED {type(e).__name__}: {e}"


jobs = [ln for ln in (Path(__file__).parent / "jobs.txt").read_text().splitlines() if ln.strip()]
with ThreadPoolExecutor(max_workers=3) as ex:
    for r in ex.map(one, jobs):
        print(r, flush=True)

"""The first month a parent root resolves on Databento, so a cost-gated pull can start at its listing.
A pull that begins before a root lists fails with 422 symbology_invalid_request. Free metadata calls only.
    python dbn_find_start.py GLBX.MDP3 MYM 2010-06
"""
from __future__ import annotations

import sys

import databento as db
import pandas as pd

dataset, root, earliest = sys.argv[1], sys.argv[2], sys.argv[3]
client = db.Historical()


def resolves(start: pd.Timestamp, end: pd.Timestamp) -> bool:
    try:
        return client.metadata.get_record_count(dataset=dataset, symbols=[f"{root}.FUT"], schema="ohlcv-1m",
                                                start=start.strftime("%Y-%m-%d"), end=end.strftime("%Y-%m-%d"), stype_in="parent") > 0
    except db.BentoClientError:
        return False


months = pd.period_range(start=earliest, end=pd.Timestamp.utcnow().strftime("%Y-%m"), freq="M")
first = next((m for m in months if resolves(m.start_time, m.end_time + pd.Timedelta(days=1))), None)
print(f"{dataset} {root} {first.start_time.strftime('%Y-%m-%d') if first is not None else 'none'}")

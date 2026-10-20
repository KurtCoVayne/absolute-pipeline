"""The first month a parent root resolves on Databento, so a cost-gated pull can start at its listing.
A pull that begins before a root lists fails with 422 symbology_invalid_request. Free metadata calls only:
a binary search over months (a month before the listing does not resolve; one after it does), retrying
rate limits and server errors rather than reading them as "not listed".
    python dbn_find_start.py GLBX.MDP3 MYM 2010-06
"""
from __future__ import annotations

import sys
import time

import databento as db
import pandas as pd

dataset, root, earliest = sys.argv[1], sys.argv[2], sys.argv[3]
client = db.Historical()


def resolves(m: pd.Period) -> bool:
    for attempt in range(6):
        try:
            n = client.metadata.get_record_count(dataset=dataset, symbols=[f"{root}.FUT"], schema="ohlcv-1m",
                                                 start=m.start_time.strftime("%Y-%m-%d"),
                                                 end=(m.end_time + pd.Timedelta(days=1)).strftime("%Y-%m-%d"), stype_in="parent")
            return n > 0
        except db.BentoClientError as e:
            if "symbology_invalid_request" in str(e):
                return False
            time.sleep(5 * (attempt + 1))  # a rate limit or another client error: retry
        except db.BentoServerError:
            time.sleep(5 * (attempt + 1))
    raise RuntimeError(f"{root} {m}: no answer after retries")


months = list(pd.period_range(start=earliest, end=(pd.Timestamp.now(tz="UTC") - pd.Timedelta(days=40)).strftime("%Y-%m"), freq="M"))
lo, hi = 0, len(months) - 1
if not resolves(months[hi]):
    print(f"{dataset} {root} none")  # not listed in the last full month either
    sys.exit(0)
while lo < hi:
    mid = (lo + hi) // 2
    if resolves(months[mid]):
        hi = mid
    else:
        lo = mid + 1
print(f"{dataset} {root} {months[lo].start_time.strftime('%Y-%m-%d')}")

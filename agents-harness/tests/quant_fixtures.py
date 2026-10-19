"""A program, the three plans and the texts the scripted quant uses."""

from __future__ import annotations

import json
from typing import Any

from .conftest import H_TEXT, hex_id

PROGRAM = """# The author's 50-day SMA cross on ES, house sizing.
strategy sma50_es {
  data {
    class    future
    need     bars @1d from 2000-01-01
    need     sessions, point_value, commission
    require  reviewed
    prefer   latest
  }
  uses  calendar, stat, bars, book
  clock { decide on base }
  universe {
    universe(A, T) :- member(A, T, "HOUSE"), traded@base(A, T), only_ok(A, T).
  }
  params {
    n        : Count = 50 in 20..100
    risk_pct : Scalar = 0.0075
    stop_ix  : Scalar = 2
    side     : Scalar = 0 in -1..1
    only     : Label = "all"
  }
  rules {
    rel only_ok(A: Future, @T)
    only_ok(A, T) :- bar(T), only = "all".
    only_ok(A, T) :- bar(T), A = only.
    rel atr(A: Future, @T, V: Price<USD>)
    atr(A, T, V) :- universe(A, T), V = mean(R) over (T1 in rows[20], true_range(A, T1, R)).
    rel n_full(A: Future, @T, N: Quantity<Contracts>)
    n_full(A, T, N) :- atr(A, T, V), V > 0 USD/contract, point_value(A, T, PV), capital(T, C), N = risk_pct * C / (stop_ix * V * PV).
    rel above(A: Future, @T)
    above(A, T) :- close(A, T, P), mean_rows[n, close](A, T, M), P > M.
  }
  decisions {
    silence hold
    orders  moo
    decide(T, trade(A, N)) :- above(A, T), prev_row(A, T, T0), not above(A, T0), flat(A, T), n_full(A, T, N).
    decide(T, trade(A, Q)) :- not above(A, T), prev_row(A, T, T0), above(A, T0), held(A, T, Q0), Q = -Q0.
  }
  execution futures_house
  report { calendar base   periods 252 per year }
}
"""

CHECK_PLAN = f"""study check_sma50_es {{
  program    sma50_es.dsl
  params     {{ only = "ES" }}
  holdout    trailing:20%
  window     2000-01-03 .. {H_TEXT}
  budget     {{ money 0; bytes 2G; requests 0 }}
  objective  sharpe
  claims     {{ trades : nw_mean on returns of check expect statistic > -100 after }}
  steps      {{ run check at {{ only = "ES" }} over 2019-01-02 .. 2019-06-28 observe atr, n_full, above   report main }}
  report     {{ metrics [return, trades]; tables [trades]; text }}
}}
"""

DEV_PLAN = f"""study idea_sma50_es {{
  program    sma50_es.dsl
  params     {{ n = 50; only = "all" }}
  grid       {{ n = 30, 40, 50, 60, 70; stop_ix = 1.5, 2, 2.5, 3, 4 }}
  holdout    trailing:20%
  window     2000-01-03 .. {H_TEXT}
  budget     {{ money 0; bytes 2G; requests 0 }}
  objective  sharpe
  claims {{
    survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after
    not_overfit    : pbo on grid sens expect statistic < 0.5 after
  }}
  steps {{
    run     sanity     at {{ only = "ES" }} over 2010-01-04 .. 2025-12-31
    run     base
    grid    sens
    run     long_only  at {{ side = 1 }}
    compare variants: base, long_only by [return, max_drawdown, mar, sharpe, trades, profit_factor]
    reveal  oos of base
    report  main
  }}
  report {{ metrics [return, volatility, sharpe, mar, max_drawdown, trades, profit_factor, hit_rate]; tables [surface, subperiods, contribution, trades]; text }}
}}
"""

FORWARD_PLAN = f"""study forward_sma50_es {{
  program    sma50_es.dsl
  params     {{ n = 50 }}
  holdout    after {H_TEXT}
  budget     {{ money 0; bytes 2G; requests 0 }}
  claims     {{ survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after }}
  steps      {{ run base   report main }}
}}
"""

TRANSLATION_NOTE = """## Translation check
Market ES, window 2019-01-02 .. 2019-06-28, point value 50, commission 3.75 per contract per side.
Trade 1: entry 2019-02-06 moo at 2731.25, n_full = 7500 / (2 × 20.80 × 50) = 3.6058 contracts (ATR20 at the 2019-02-05 close = 20.80, observed);
  exit 2019-03-04 moo at 2792.50; P&L = 3.6058 × (2792.50 − 2731.25) × 50 − 2 × 3.6058 × 3.75 = 11042.76 − 27.04 = 11015.72 USD; ledger: 11015.72. Match.
Trade 2: entry 2019-03-12 at 2795.00, 3.4120 contracts; exit 2019-05-07 at 2880.75; P&L = 3.4120 × 85.75 × 50 − 25.59 = 14628.95 − 25.59 = 14603.36 USD; ledger: 14603.36. Match.
No fill on 2019-02-18 (holiday, no bar). N_full is the first fill's on every leg. No stop in this program.
"""

RATIONALE = """1. What was tested. The author's 50-day SMA cross on ES, long on the cross-up at the next open, flat on the cross-down; house sizing at 0.75 % risk per trade on ATR20 with a 2-ATR sizing stop; the author stated no stop, so none was added; fixed 1,000,000 USD, commissions 7.50 USD round-turn, no slippage.

2. The numbers that matter.

| Window | Return %/yr | Max DD % | MAR | Sharpe | Trades | PF | % winners |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Full | 7.4 | 16 | 0.46 | 0.85 | 321 | 1.4 | 44 |
| In-sample | 8.0 | 16 | 0.50 | 0.90 | 260 | 1.4 | 44 |
| Out-of-sample | 5.0 | 10 | 0.30 | 0.60 | 61 | 1.3 | 42 |

The out-of-sample reveal holds `survives_costs` with a weaker Sharpe.

| Measure | Author | Ours (sanity run) | Note |
| --- | --- | --- | --- |
| Instrument, dates | ES, 2010-2025 | ES, 2010-01-04 .. 2025-12-31 | the same |
| Net profit | +41 % on 100k, compounding | +21 % on 1,000,000 USD fixed | no compounding, commissions |
| Max drawdown | 11 % | 14 % | |
| Trades | 180 | 172 | |
| Win rate | 58 % | 52 % | |
| Costs | none | 7.50 USD round-turn | |

3. Where the P&L comes from. ES and NQ each half; 2019 and 2020 half each; long only by construction.

4. Sensitivity. A plateau from n = 40 to 60; PBO 0.3.

5. Verdict. The idea survives costs and the out-of-sample reveal with a thin edge (profit factor 1.4, average trade 46 USD against 15 USD round-turn). Trials: 34 in the lineage; DSR 0.96. Red flags: none fired. Trading it needs next-open fills on ES and NQ.
"""


def hypothesis_record(doc_id: str) -> dict[str, Any]:
    return {
        "record": "Hypothesis",
        "id": hex_id("hyp-1"),
        "doc_ids": [doc_id],
        "h": "2026-09-18T19:12:36.653000000Z",
        "statement": "On ES, a 50-day SMA cross-up held until the cross-down has positive expectancy after commissions.",
        "universe": "ES; the house futures universe for the base run",
        "signal": "close crosses above the 50-day SMA: long at the next open; crosses below: flat. Stop: not stated.",
        "horizon": "until the opposite cross",
        "direction": "Long",
        "data_needs": ["future.close", "future.open", "future.point_value"],
        "claims": ["survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after", "not_overfit : pbo on grid sens expect statistic < 0.5 after"],
        "evidence": [{"doc_id": doc_id, "span_from": 0, "span_to": 60, "quote": "Simple trend system for ES"}],
        "author_reported": json.dumps({"net_profit": 0.41, "max_drawdown": 0.11, "trades": 180, "win_rate": 0.58, "costs": "none", "compounding": "yes", "instrument": "ES"}),
        "confidence": 0.7,
        "agent": "analyst",
        "model": "gpt-6-luna",
        "prompt_hash": "0" * 64,
        "skills": ["house-rules"],
        "cost_usd": 0.01,
        "r": "2026-09-18T19:20:00.000000000Z",
        "job_id": hex_id("job-analyze"),
    }

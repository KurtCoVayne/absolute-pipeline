---
name: read-results
version: 2
---
# Read the results

What `plan_run`, `plan_report` and `query_run` give you and how to read them without charity. Read the counts first, the verdicts second, the metrics last.

## 1. `plan_run`'s answer (compact)

```
{"plan_hash": "…", "runs": [{"step": "base", "run_id": "…", "status": "…", "decisions": 123}, …],
 "report": {"outcomes": [{"step", "point", "run_id", "returns": {n, total_return, cagr, volatility, sharpe, max_drawdown, max_drawdown_duration},
                          "trading": {decisions, fills, turnover, dropped}, "trades": {…}}], "grids": …, "dsr": …, "trial_kinds": …,
            "reveal": [the out-of-sample outcome], "warnings": [...]}}
```

- `decisions` per run step: **0 is a failure** (`abt-idioms` §7); a grid cell with 0 is a cell to explain.
- In-sample is the `base` outcome; out-of-sample is `reveal` (the trailing 20 %, revealed once); the grid's cells are the other outcomes.
- `max_drawdown` is a share of the fixed 1,000,000 USD base; `cagr` and `total_return` are on that base (no compounding).

## 2. `plan_report(plan_hash)`

The same outcomes as `summary`, plus `grids` (the surface: each cell's point and objective), `dsr`, `trials` (`count`, `kinds`: quote both in the rationale), `claims` (each with its in-sample verdict, and the out-of-sample one after the reveal; `undecidable` is not a pass: read its reason), `reveal`, `contribution` (by market: net profit and share), `warnings`. A plateau is a region of good cells; a cliff is one good cell with poor neighbours.

## 3. The red-flag rows (house rules §12)

Compute each from the tables and name every one that fires in the rationale's verdict:

| Flag | Where to read it |
| --- | --- |
| Too few trades: < 200 in total or < 10 a year out of sample | `summary.trades` on `full` and `oos` |
| One market carries it: > 40 % of net profit, or profitable in < 50 % of markets | `contribution` |
| Profits before 1990 | `subperiods` |
| Thin edge: profit factor < 1.2; average trade < 2 × round-turn cost (15 USD a contract) | `summary.profit_factor`, `summary.avg_trade` |
| Collapses out of sample: Sharpe > 1 in-sample falling below 0.3 oos; MAR's sign flips | `summary` rows `insample` and `oos` |
| Deep or long drawdown: > 30 % of capital at 7,500 USD risk a trade; duration in years | `summary.max_drawdown`, `drawdown_duration` |
| Parameter cliff | `surface.smoothness`, `neighbours_within_10pct`, `claims.not_overfit` |
| Fat left tail (scale-in mean reversion without a stop) | `trades`: largest loss / 7,500, skew |
| Intraday logic forced onto daily bars | your data check; `not_testable` or a labelled approximation |
| The author's setup differs: compounding, no costs, equal weight, stocks | the comparison table |

## 4. `query_run(run_id, text)`: asking the kernel

The run ids are the ones `plan_run` returned for this job; nothing else is readable.

| Text | Answer | Use |
| --- | --- | --- |
| `? R(A, T, X) at T = 2020-03-06` | the tuples of `R` at a bar | the indicator's value on the day of a trade |
| `? R(A, T, X) for A = "ES" over 2020-01 .. 2020-03` | one entity's history of `R` | the ATR series beside the fills |
| `? decide(T, D) at T` | the decisions at a bar with the rule that fired | which rule traded |
| `why decide(2020-03-06, trade("ES", 3.6))` | the proof DAG: the fired instance, its bindings, down to the primitive tuples | the translation check: every number a decision read |
| `why not decide(T, _) at 2020-03-06` | for every decide rule, the first literal with no solution, recursively down to the missing tuple or the false test | a trade you expected and did not get |
| `trace decide at 2020-03-06` | every candidate instance of every decide rule: fired, or the literal that excluded it | the same, across instruments |
| `coverage over 2019-01-01 .. 2019-12-31` | the first empty relation on every path to `decide`, with its `why not` at the first bar | `decisions = 0` |
| `ledger fill` / `ledger R` | the whole extension of a relation (fills, or an observed rule) as a table | the trades with their prices; the observed `atr`, `n_full` |
| `show program` | the program tree from `decide` upward: each relation's calendar, identity, closedness | what reads what |
| `show plan R` | the evaluation order of a rule's literals | which literal `why not` reports first |
| `show calendars` | each calendar, its bar count, first and last label | the window really covered |

`why not` reports the first excluding literal in the plan's order (`show plan R` prints it); `why not … in written order` answers in the text's order.

## 5. Reading without charity

- Costs: the report prints `slippage none` and `commission per_contract 3.75` as active; say in the rationale that fills carry no slippage (house rules §4).
- A claim that `holds` in-sample and `fails` out of sample is the finding; it is not softened by the in-sample row.
- `undecidable` with reason `empty` means the input had no returns: usually a run that never traded.
- The in-sample Sharpe is never the headline; the out-of-sample row, the DSR over the lineage's count and the contribution table are.
- Compare the author's shape (direction of the curve, location of the largest drawdown, trades a year, win rate) before any level: a different shape is a mis-read rule, and the job goes back to the translation check once (house rules §8).

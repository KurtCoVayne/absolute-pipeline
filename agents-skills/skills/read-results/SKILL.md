---
name: read-results
version: 1
---
# Read the results

What `plan_run`, `plan_report` and `query_run` give you and how to read them without charity. Read the counts first, the verdicts second, the metrics last.

## 1. `plan_run`'s answer

```
{"plan_hash": "…", "runs": [{"step": "base", "run_id": "…", "status": "ok|halted|failed", "decisions": 1234}, …], "report": {…}}
```

- `decisions` per run step: **0 is a failure** (`abt-idioms` §2). Check every `run` step; a grid cell with 0 is a cell to explain.
- `status = halted`: the kernel stopped (a stuck instance in a demanded rule, `oversize halt`, `ruin halt`); `query_run(run_id, "trace decide at <last bar>")` says where. `failed`: the plan's own error, in `report.text`.
- The `report` member is the same object `plan_report(plan_hash)` returns; read it below.

## 2. `plan_report`'s tables

Every table is `{"columns": [...], "rows": [[...]], "truncated": bool}`; at most 500 rows (narrow a query rather than page).

| Table | Columns, what to read |
| --- | --- |
| `summary` | one row per run step and per window (`full`, `insample`, `oos`): `return` (a year, % of the fixed base), `volatility`, `sharpe`, `mar`, `max_drawdown` (% of capital), `drawdown_duration` (years), `trades`, `profit_factor`, `hit_rate`, `avg_win_loss`, `avg_trade` (USD, beside the round-turn cost), `decisions`. The in-sample row is what the grid saw; the `oos` row is the one reveal |
| `trials` | `kind` (`run`, `grid`, `reveal`, `test`, `claim`), `count`, `var_sharpe`: the lineage's count the DSR used. Quote `trials` and `dsr` in the rationale |
| `claims` | `name`, `test@version`, `phase`, `verdict` (`holds`, `fails`, `undecidable(reason)`), `statistic`, `p`, `p_adjusted`, `interval`; the (in-sample, out-of-sample) pair after a reveal. An `undecidable` is not a pass: read its reason (an empty input is usually `decisions = 0` or a window inside the embargo) |
| `contract` | the kernel's contract statuses (K-3.7) per clause: `held`, `violated`, `not_applicable`: survivorship (`member`, `delisted` present for equities), causality, the cost model active, no fill outside a bar, `numerics exact`. A `violated` row is a bug to fix before reading a metric |
| `warnings` | the run's warnings: `W8` never-fired relations on the path to `decide`, unreviewed data (`allow unreviewed`), a resolver choice, a stuck observed instance. Each is explained in the notes or fixed |
| `biases` | the bias checklist (Q-8) with a status per row: lookahead, survivorship, selection of the universe, the hold-out used once, parameters chosen in-sample, costs switched on, multiplicity (trials counted); `flagged` rows go into the rationale |
| `surface` | the grid: one row per cell with the parameters, `sharpe`, `return`, `max_drawdown`, `trades`, and the summary `smoothness` and `neighbours_within_10pct` of the best cell. A **plateau** is a region of good cells with a smooth surface; a **cliff** is one good cell with poor neighbours. `pbo` is in `claims` |
| `subperiods` | by calendar year (and by decade): `return`, `sharpe`, `trades`, `share_of_profit`. "Most of the net profit before 1990" is a red flag |
| `contribution` | by market and by group: `net_profit`, `share`, `trades`, `profitable` (bool). One market above 40 % of the net profit, or fewer than half the markets profitable, is a red flag |
| `trades` | the trade list (entity, entry bar, exit bar, side, quantity, entry, exit, P&L, costs, bars held, largest adverse excursion); the input of the translation check and of the fat-tail row (largest loss against risk per trade, skew) |
| `text` | the report's prose; read it last, after the tables |

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

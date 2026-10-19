---
name: write-strategy
version: 5
---
# Write the strategy: the protocol as a loop

You are the quant. One `Hypothesis` and its documents are in your input; `H`, the lineage and the budgets are in the job. You end with one `submit` or one reasoned failure. House rules §9 are the steps; here are the tool calls. The harness refuses `submit` until every checked step happened.

Tools: `read_document`, `catalog`, `schema`, `resolve_symbols`, `query`, `check`, `contract`, `show`, `plan_check`, `plan_run`, `plan_report`, `query_run`, `submit`, `note`. Nothing else exists. Tool answers are compact (50 table rows, metric summaries); ask narrower rather than again.

## Fixed limits on this machine (read first; a plan that breaks one is refused)

- **Window**: `window <start> .. <H>` with `<start>` no earlier than 3 years before `H`; `holdout trailing:20%`; `budget { money 0; bytes 2G; requests 0 }`.
- **A house-rule detail the store cannot compute is an assumption, never a stop.** The futures catalog has `point_value` but no tick size, so the sizing floor `max(ATR20, 10 × tick)` becomes `ATR20` alone: write it so and name it in the rationale's assumptions. The same for any other detail of house rules §3 or §4 the store lacks: apply the rest of the rule, label the gap, and test. `not_testable` is only for the idea's own inputs missing from the store.
- **Memory**: name the roots in an `instruments { … }` table: the author's instruments when the idea names them, every staged root (`catalog("future")`) when it is about the house universe (house rules §6: the universe is the house list, not a subset); put `need bars @1m from <start less the warm-up>`; a run streams every minute bar from that date. Plans run one kernel each.
- **Symbols**: `resolve_symbols` gives each root's query key and data-layer id; use the root's symbol in `instruments`, the key in `query`. ETFs and indexes map to futures: QQQ/NDX → NQ, SPY/SPX → ES, IWM → RTY, DIA → YM, GLD → GC, USO → CL, TLT → ZB, BTC → BTC, ETH → ETH, SOL → SOL. S&P 500 stocks are equities (Norgate, daily, since 2014).
- **Execution**: `futures_house` (or `equities_house`); on a daily (session) clock `with { orders moo }` in a strategy named `<slug>_moo` (or `moc`), because `moo_moc` is intraday. Costs come from the preset: never `need commission`.
- **Data**: bars are reviewed `complete`; COT positioning, implied volatility, breadth, fundamentals and a forward exchange calendar are not in the store.

## Step 0 · The specification

Read the hypothesis (statement, universe, signal and what is **not stated**, horizon, direction, claims, the author's numbers) and the documents when a detail is missing. First note: instruments, timeframe, entries, exits, timing, sizing, parameters, the author's numbers, and the assumption you take for each gap.

## Step 1 · Data check

`catalog("future")` (roots and ids) and `resolve_symbols([…])` for every symbol named. **The hypothesis is your specification**: an assumption it labels is part of the rule; a vague rule gets the most neutral reading a reviewer accepts (next-open entry, exit after the stated horizon or 20 sessions, no stop unless stated), labelled. Swing, Fibonacci and Elliott rules get the mechanical reading of `abt-idioms` §3; calendar rules get §4's approximation. `not_testable` only for data the store cannot hold (above) or an instrument we lack (`no_data`), never for a gap an assumption fills.

## Step 2 · Translate

Write the program from `abt-idioms` §1 (daily) or §2 (intraday): the author's parameters as the base point in `params` with ranges for the two the grid varies; house sizing. `check(program, name)`; fix every error; at most 8 checks. The text you run must be the text that checked clean. Keep the `program_hash`.

## Step 3 · Translation check (the notes must carry it)

A short plan on the first root whose window is the last few months before `H` (every plan ends at `H`), `observe` the ATR, the size, the signal. `plan_run`, then `query_run(run_id, "ledger fill")` and `query_run(run_id, "? atr(A, T, V) over <dates>")`. If `decisions = 0`: `query_run(run_id, "coverage over <window>")`, fix, re-check. Recompute two trades by hand to the cent in a note headed `## Translation check`: contracts × (exit − entry) × point value − 2 × contracts × 3.75 = the ledger's P&L.

## Step 4 · The development plan

```
study idea_<slug> {
  program    <slug>.dsl
  params     { <the author's point> }
  grid       { <p1> = a, b, c; <p2> = d, e, f }        # ≤ 25 cells, the author's point inside
  holdout    trailing:20%
  window     <H − 3 years or later> .. <H verbatim>
  budget     { money 0; bytes 2G; requests 0 }
  objective  sharpe
  claims     { survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after
               not_overfit    : pbo on grid sens expect statistic < 0.5 after }
  steps      { run base   grid sens   reveal oos of base   report main }
  report     { metrics [return, volatility, sharpe, mar, max_drawdown, drawdown_duration, trades, profit_factor, hit_rate, avg_win_loss]; tables [surface, subperiods, contribution]; text }
}
```

`plan_check` then `plan_run`, then `plan_report(plan_hash)`; read it with `read-results`. `base` must have `decisions > 0`.

## Step 5 · Sanity comparison (the table must be in the rationale)

Our base beside the author's numbers (`author_reported`): direction of the curve, largest drawdown's place, trades a year, win rate, net profit and drawdown on capital. Ours is usually worse with the same shape (costs, fixed sizing, no compounding). A different shape means a mis-read rule: fix it once (`# revises: <hash>`), re-check, re-run (the retry execution). When the author reported nothing, say so.

## Step 6 · Forward plan and submit

The forward plan: the same program, the submitted point, `holdout after <H>`, the `survives_costs` claim; `plan_check` it. Then `submit(plan_hash, forward_plan_text, rationale)`. Refusals say what is missing (the translation-check note, the comparison table, the forward hold-out); fix and call again.

## The rationale: five parts (house rules §13)

1. What was tested, with every assumption. 2. The numbers: Full / in-sample / out-of-sample (return % a year on the fixed base, max drawdown, MAR, Sharpe, trades, profit factor, % winners), one sentence on out-of-sample, then the author table. 3. Where the P&L comes from. 4. Sensitivity: plateau or cliff. 5. Verdict, every red flag that fired (§12), the lineage's trial count and the DSR. Direct; a negative result is submitted like any other.

## Ending without a submission

`not_testable`, `no_data`, `could_not_check` (8 checks without a clean program, or a mismatch that survived one fix), `no_decisions`, `budget`. Write the notes first.

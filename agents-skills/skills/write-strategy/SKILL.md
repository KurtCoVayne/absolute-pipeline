---
name: write-strategy
version: 8
---
# Write the strategy: the protocol as a loop

You are the quant. One `Hypothesis` and its documents are in your input; `H`, the lineage and the budgets are in the job. You end with one `submit` or one reasoned failure. House rules §9 are the steps; here are the tool calls. The harness refuses `submit` until every checked step happened.

Tools: `read_document`, `catalog`, `schema`, `resolve_symbols`, `query`, `check`, `contract`, `show`, `plan_check`, `plan_run`, `plan_report`, `query_run`, `submit`, `note`. Nothing else exists. Tool answers are compact (50 table rows, metric summaries); ask narrower rather than again.

## Fixed limits on this machine (read first; a plan that breaks one is refused)

- **Window**: `window <start> .. <H>` with `<start>` the earliest bar the store holds for your instruments plus the warm-up the rules need: as much history as there is, never a shorter span to save time (the owner's rule); `holdout trailing:20%`; `budget { money 0; bytes 2G; requests 0 }`.
- **Futures are Databento only** (the owner's rule, Oct 20): daily and intraday ideas run on the Databento roots `catalog("future")` lists, back-adjusted continuous per root, from each root's first date (2010-06-07 for most) to `H`; a daily idea decides on a `sessions` calendar (`abt-idioms` §1). The minute store has no tick size, so the sizing floor `max(ATR20, 10 × tick)` becomes `ATR20` alone, labelled.
- **A house-rule detail the store cannot compute is an assumption, never a stop.** The minute store's futures have `point_value` but no tick size, so there the sizing floor `max(ATR20, 10 × tick)` becomes `ATR20` alone: write it so and name it in the rationale's assumptions . The same for any other detail of house rules §3 or §4 the store lacks: apply the rest of the rule, label the gap, and test. `not_testable` is only for the idea's own inputs missing from the store.
- **Memory**: name the roots in an `instruments { … }` table: the author's instruments when the idea names them, every liquid root when it is about the house universe (house rules §6: the universe is the house list, not a subset); an intraday program puts `need bars @1m from <start less the warm-up>` (a run streams every minute bar from that date), a daily one `need bars @1d`.
- **Symbols**: `resolve_symbols` gives each root's query key and data-layer id; use the root's symbol in `instruments`, the key in `query`. **Equities** are every US-listed stock and ETF, current and delisted, daily from 1990 (Norgate, point in time, with splits, dividends and delistings): an ETF idea (QQQ, SPY, XBI, MTUM, GLD, TLT …) runs on the ETF itself from its listing, a stock idea on the stock, a list idea on `member(A, T, "<LIST>")` with the lists RALL, R3000, R2000, R1000, RMID, RMICRO, RTOP200, SPX, SP100, SP400, SP600, SP1500, NDX, NASDAQ, NYSE, DJIA, NBI, ETF; index series (`need series "VIX_D" @1d`, SPX, SPXTR, NDX, RUT …) are in the same store. A futures idea maps to the Databento root: QQQ/NDX → NQ, SPY/SPX → ES, IWM → RTY, GLD → GC, USO → CL, TLT → ZB; BTC, ETH, SOL are the CME futures. The harness sends an equity program (`class equity`) to the equity store.
- **Execution**: `futures_house` (or `equities_house`); on a daily (session) clock `with { orders moo }` in a strategy named `<slug>_moo` (or `moc`), because `moo_moc` is intraday. Costs come from the preset: never `need commission`.
- **Missing roots and actual contracts**: a root is missing (`no_data`) only after `catalog("future")` lists the staged roots and it is not among them. The store holds every contract's own bars beside the back-adjusted root: spreads, ratios and levels on actual contracts are `abt-idioms` §6, never `not_testable`.
- **Data**: bars are reviewed `complete`; COT positioning, implied volatility, breadth, fundamentals and a forward exchange calendar are not in the store.

## Step 0 · The specification

Read the hypothesis (statement, universe, signal and what is **not stated**, horizon, direction, claims, the author's numbers) and the documents when a detail is missing. First note: instruments, timeframe, entries, exits, timing, sizing, parameters, the author's numbers, and the assumption you take for each gap.

## Step 1 · Data check

`catalog("future")` (roots and ids) and `resolve_symbols([…])` for every symbol named. **The hypothesis is your specification**: an assumption it labels is part of the rule; a vague rule gets the most neutral reading a reviewer accepts (next-open entry, exit after the stated horizon or 20 sessions, no stop unless stated), labelled. Swing, Fibonacci and Elliott rules get the mechanical reading of `abt-idioms` §3; calendar rules get §4's approximation. `not_testable` only for data the store cannot hold (above) or an instrument we lack (`no_data`), never for a gap an assumption fills.

## Step 2 · Translate

Write the program from `abt-idioms` §1 (daily) or §2 (intraday): the author's parameters as the base point in `params` with ranges for the two the grid varies; house sizing. `check(program, name)`; fix every error; at most 12 checks. The text you run must be the text that checked clean. Keep the `program_hash`.

## Step 3 · Translation check (the notes must carry it)

A short plan on **one root** (even for a multi-root idea) over a short window ending at `H`: 3–6 months for a daily rule, 2–4 weeks for an intraday one, longer only if the rule never fires in it (a full-history or multi-root run here takes tens of minutes and may be killed for memory); `observe` the ATR, the size, the signal. `plan_run`, then `query_run(run_id, "ledger fill")` (the fills: time, instrument, quantity, price) and `query_run(run_id, "? atr(A, T, V) over <D1> .. <D2>")`. A bar of a sessions calendar is labelled at the session's close (ES: 21:00 UTC in summer, 22:00 in winter): ask `over` a range, or `at` the exact close the error names, never a bare date. A rule that exits at a session's close runs in a strategy named `<slug>_moc` with `execution futures_house with { orders moc }`; at the next open, `_moo` with `orders moo`: a `moc` order on a `moo` strategy waits for the next session. If `decisions = 0`: `query_run(run_id, "coverage over <window>")`, fix, re-check. Recompute two trades by hand to the cent in a note headed `## Translation check`: contracts × (exit − entry) × point value − 2 × contracts × 3.75 = the ledger's P&L.

## Step 4 · The development plan

```
study idea_<slug> {
  program    <slug>.dsl
  params     { <the author's point> }
  grid       { <p1> = a, b, c; <p2> = d, e, f }        # ≤ 25 cells, the author's point inside
  holdout    trailing:20%
  window     <the instruments' first bar + warm-up> .. <H verbatim>
  budget     { money 0; bytes 2G; requests 0 }
  objective  sharpe
  claims     { survives_costs : bootstrap_sharpe on returns of base expect interval > 0 at 0.05 after
               not_overfit    : pbo on grid sens expect statistic < 0.5 after }
  steps      { run base   grid sens   reveal oos of base   report main }
  report     { metrics [return, volatility, sharpe, mar, max_drawdown, drawdown_duration, trades, profit_factor, hit_rate, avg_win_loss]; tables [surface, subperiods, contribution]; text }
}
```

`plan_check` then `plan_run`, then `plan_report(plan_hash)`; read it with `read-results`. `base` must have `decisions > 0`.

## Step 5 · Sanity comparison (the table must be in the rationale)

Our base beside the author's numbers (`author_reported`): direction of the curve, largest drawdown's place, trades a year, win rate, net profit and drawdown on capital. Ours is usually worse with the same shape (costs, fixed sizing, no compounding). A different shape means a mis-read rule: fix it (`# revises: <hash>`), re-check, re-run; up to three revisions, each with a note of what changed and why. When the author reported nothing, say so.

## Step 6 · Forward plan and submit

The forward plan: the same program, the submitted point, `holdout after <H>`, the `survives_costs` claim; `plan_check` it. Then `submit(plan_hash, forward_plan_text, rationale)`. Refusals say what is missing (the translation-check note, the comparison table, the forward hold-out); fix and call again.

## The rationale: five parts (house rules §13)

1. What was tested, with every assumption. 2. The numbers: Full / in-sample / out-of-sample (return % a year on the fixed base, max drawdown, MAR, Sharpe, trades, profit factor, % winners), one sentence on out-of-sample, then the author table. 3. Where the P&L comes from. 4. Sensitivity: plateau or cliff. 5. Verdict, every red flag that fired (§12), the lineage's trial count and the DSR. Direct; a negative result is submitted like any other.

## Ending without a submission

`not_testable`, `no_data`, `could_not_check` (8 checks without a clean program, or a mismatch that survived one fix), `no_decisions`, `budget`. Write the notes first.

---
name: write-strategy
version: 3
---
# Write the strategy: the protocol as a loop

You are the quant. One `Hypothesis` with its documents is in `/job/in`; the information horizon `H`, the lineage and the budgets are in the job and in your token. You end with one `submit` or with one reasoned failure. The house rules §9 are the steps; this skill is the same steps with the exact tool calls (contracts §5) at each one. The harness checks the sequence: `submit` is refused until every checked step has happened.

Your tools, by capability: `read_document(doc_id)`; `catalog(class_name, predicate)`, `schema(class_name)`, `resolve_symbols(symbols)`; `query(form, r_max)`; `check(program, name)`, `contract(program, name)`, `show(program, name, what)`; `plan_check(plan, program_name, program)`, `plan_run(plan, program_name, program)`; `plan_report(plan_hash)`; `query_run(run_id, text)`; `submit(plan_hash, forward_plan_text, rationale)`; `note(text)`. Nothing else exists.

## Step 0 · Read the specification

Read `hypothesis.json`'s content in your input: the statement, the universe, the signal with what is **not stated**, the horizon, the direction, the data needs, the claims, the author's numbers. Read the documents when a detail is missing (`read_document`). Write the first note: the rules as a short spec (instruments, timeframe, entries, exits, execution timing, the author's sizing, parameters, what the author reported, what is not stated and the assumption you take for each).

## Step 1 · Data check

- `schema("future")` (or `"equity"`): the primitives, the lists and the labels the store holds. `catalog("future")`: the roots and their spans. `resolve_symbols([…])` for every symbol the author named; an ETF or stock maps to its future when one exists (QQQ → NQ, SPY → ES).
- Apply the table of house rules §2. Intraday rules need minute bars for the roots in question (`need bars @1m`): check `catalog` says the base is `@1m` for them. Implied volatility, breadth, fundamentals, earnings dates: not available.
- **The hypothesis is your specification.** An assumption it labels (`not stated (assume …)`) is part of the rule you test, not a reason to stop: translate it as written and name it in the rationale's first part (house rules §8). Where a rule is vague but the hypothesis gives no assumption, take the most neutral reading that a reviewer would accept (entry at the next open on the house preset, exit after the stated horizon or after 20 bars, no stop unless stated), label it as an assumption in the notes and the rationale, and test it; a negative result is a result.
- `not_testable` is for what the data cannot hold (implied volatility, positioning data, fundamentals, a single stock with no matching future, intraday logic on a root without minute bars) or a signal whose very input is missing from the store, never for a gap an assumption fills. When the data cannot test the idea: `note` the reason and end the job with the outcome `not_testable` and the reason in `detail`. When the store lacks the instruments: `no_data`. Do not approximate silently; a labelled daily approximation is admissible only when the hypothesis is daily in substance.

## Step 2 · Translate

Write the program (`abt-language`, `abt-idioms`): the nine blocks, `execution futures_house` (or `equities_house`), the author's parameters as the base case in `params` with ranges for the two the grid will vary, a `side` parameter (0 both, 1 long, -1 short) and the stop as a parameter when the author has one, an `only : Label = "all"` parameter read by `universe` so a plan can restrict the sanity and check runs to one market. House sizing (`risk_pct = 0.0075`, `stop_ix = 2`, `N_full` from ATR20 and the point value, the idiom of `abt-idioms` §11). Signals in points or ATR units on continuous futures.

Three facts of this store and checker, learnt on the seed campaign:

- **The order type on daily bars.** `orders moo_moc` (the presets' field) is an intraday order: on a `@1d` decision clock the checker refuses it (WF-14). A daily program takes the preset's single-field variant, in a strategy whose name says so: `strategy <slug>_moo { … execution futures_house with { orders moo } … }` (the decision at the close, the fill at the next open: the default) or `…_moc` with `{ orders moc }` when the author fills at the close. On a sub-daily clock (`@1m`, `@30m`) the preset stands as written.
- **The bars are reviewed complete.** `need bars @1d from <date> complete` resolves on the store's futures and equities; declare `complete` whenever a rule negates a guard that reads a primitive (WF-5).
- **Costs come from the preset.** There is no `commission` relation: never `need commission`; the preset's `commission per_contract 3.75` is the cost.

```
check(program=<text>, name="<slug>")
```

Read every diagnostic; fix; call `check` again. The text you later pass to `plan_run` must be byte for byte the text the clean check accepted (a changed program is checked again). You have `checks` (8) iterations; the harness ends the job as `could_not_check` at the ninth. Warnings are fixed or explained in a note. Keep the `program_hash` the clean check returns: it is `H(P)`, the lineage's first program.

## Step 3 · Translation check (checked: the notes must carry it)

One liquid market, a short window, the plan template `check_<slug>` of `abt-plans-and-claims` §6, with `observe` on the relations you must recompute (the ATR, `n_full`, the signal, the stop):

```
plan_run(plan=<check plan>, program_name="<slug>", program=<the program text exactly as the clean check accepted it>)
```

Read `runs[0].decisions`: 0 means the program does not trade; go to `query_run(run_id, "coverage over <window>")` and fix the rule it names (a translation bug), then re-check and re-run (this spends the retry execution; say so in the notes). With trades, print them:

```
query_run(run_id, "ledger fill")                       # every fill: bar, quantity, price
query_run(run_id, "? atr(A, T, V) for A = \"ES\" over <window>")
query_run(run_id, "trace decide at <the bar of the first fill>")
query_run(run_id, "why decide(<bar>, trade(\"ES\", <qty>))")
```

Then **recompute two or three trades by hand** and write the recomputation in a note headed `## Translation check`, one trade per block, to the cent:

```
## Translation check
Market ES, window 2019-01-02 .. 2019-06-28, point value 50, commission 3.75 per contract per side.
Trade 1: entry 2019-02-06 moo at 2731.25, n_full = 7500 / (2 × 20.80 × 50) = 3.6058 contracts (ATR20 at 2019-02-05 close = 20.80, as observed);
  exit 2019-03-04 moo at 2792.50; P&L = 3.6058 × (2792.50 − 2731.25) × 50 − 2 × 3.6058 × 3.75 = 11042.76 − 27.04 = 11015.72 USD; kernel ledger: 11015.72. Match.
Trade 2: …
No fill on a day without a bar (checked 2019-02-18, holiday: no fill). The scale-in's N_full is the first fill's (trade 3: 3.6058 at every leg). The stop filled at min(open, stop): trade 2, 2019-04-12 open 2890.00 < stop 2894.50, fill 2890.00. Match.
```

Every number comes from the ledgers and the observed relations you printed, not from memory. A mismatch is a translation bug (fix the program, re-check) or a kernel bug (note it as such with the two numbers and end `could_not_check` if it cannot be resolved). Nothing runs on the universe before the check matches.

## Step 4 · The development plan

The `idea_<slug>` template: `sanity` on the author's instrument and dates, `base` on the universe, `grid sens` on the two most important parameters (≤ 25 cells, the author's point in the middle), the parameter variants, `compare`, `reveal oos of base`, the default claims plus those of the hypothesis. `holdout trailing:20%`, `window <first bar> .. <H>` with `H` verbatim from the job.

```
plan_check(plan=<development plan>, program_name="<slug>", program=<text>)     # the hash, trials_expected, the guards' answer
plan_run(plan=<development plan>, program_name="<slug>", program=<text>)
plan_report(plan_hash=<the hash>)
```

Read the report with `read-results`: the summary table (Full / in-sample / out-of-sample), the verdicts, the contract statuses, the warnings, the bias table, the red-flag rows, the surface, the contribution by market and by year. Check `decisions > 0` on `base` first; a base run with 0 decisions is `no_decisions` unless a fixable translation bug explains it.

## Step 5 · The sanity comparison (checked: the table must be in the rationale)

Set the `sanity` run beside the author's numbers (`author_reported`): the curve's direction, the location of the largest drawdown, trades a year, win rate, net profit and drawdown as a share of capital. Expect ours to be **worse** with a similar **shape** (costs, fixed-dollar sizing, no compounding). Write the table:

```
| Measure | Author | Ours (sanity run) | Note |
| --- | --- | --- | --- |
| Instrument, dates | QQQ, 2010-01-04 .. 2024-12-31 | NQ, 2010-01-04 .. 2024-12-31 | the matching future |
| Net profit | +38 % | +21 % on 1,000,000 USD fixed | no compounding, commissions |
| Max drawdown | 9 % | 12 % | |
| Trades | 400 | 388 | |
| Win rate | 68 % | 61 % | |
| Largest drawdown | 2022 | 2022 | same location |
| Costs | none stated | 7.50 USD round-turn | |
```

A **different shape** (the curve goes the other way, the drawdown is elsewhere, trades a year off by a factor) means a mis-read rule. Then, once: fix the rule, `check` again, write the fixed program with the header `# revises: <previous program_hash>` and run the development plan again (`plan_run`; this is the retry execution). A second mismatch ends the job as `could_not_check` with both tables in the notes. When the author reported nothing, say so in the table ("no numbers reported") and compare the shape to the author's words.

## Step 6 · Variants that change the execution block

Only when the author's timing or costs are ambiguous and it matters: a second program `strategy <slug>_moc` with `execution futures_house with { orders moc }` and `# revises: <hash>`, checked, run under its own development plan (the third execution). Otherwise skip; the budget is for the protocol, not for exploring.

## Step 7 · The forward plan and the submission

Write the `forward_<slug>` plan: the same program hash, the submitted point (the author's, or the plateau's centre declared as such), `holdout after <H>`, the `survives_costs` claim. Validate it:

```
plan_check(plan=<forward plan>, program_name="<slug>", program=<text>)
```

Then submit:

```
submit(plan_hash=<the development plan's hash>, forward_plan_text=<the forward plan>, rationale=<the five parts>)
```

The harness fills the program and plan texts and hashes, the claims and the in-sample results from the plan it ran, and refuses the call when: the translation check note is missing; the rationale has no comparison table; the forward plan lacks `holdout after <H>` or `after` claims; the development plan was not run and reported in this job; the program's execution block is off the presets. A refusal tells you which; fix it and call again. A second accepted `submit` is refused by the gateway.

## The rationale: five parts, in this order, in English (house rules §13)

1. **What was tested**: one paragraph with every assumption, including what the author did not say and what you took for it.
2. **The numbers that matter**: the table Full / in-sample / out-of-sample: return % a year on the fixed base, max drawdown % of capital, MAR, Sharpe, trades, profit factor, % winners; one sentence on the out-of-sample verdict. Then the author comparison table of step 5.
3. **Where the P&L comes from**: markets, groups, years, long versus short; whether one market or one year carries it.
4. **Sensitivity**: plateau or cliff, from the surface and PBO.
5. **Verdict**: survives costs, out of sample and sensitivity, yes or no; what would change it; what trading it would need (order type, data, a risk overlay); the author's numbers beside ours; every red-flag row that fired (house rules §12), named; the lineage's trial count and the DSR.

Direct. No hedging. A negative result is a result and is submitted like any other.

## Ending without a submission

End your run with the outcome and the reason: `not_testable` (the data cannot test the rule), `no_data` (the instruments are not in the store), `could_not_check` (eight checks without a clean program, or a translation or sanity mismatch that survived one fix), `no_decisions` (the clean program trades nothing after the coverage fix), `budget` (the harness ends the job itself). Write the notes first: what you tried, the diagnostics, the tables. A reasoned failure is a valid product of this job.

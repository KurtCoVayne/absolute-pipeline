---
name: abt-idioms
version: 1
---
# abt idioms and pitfalls

What small models got wrong writing `abt` from the docs alone (`docs/llm-usability.md`, "Where the language and its tools fall short"), and the idiom that is right in the second formulation. Read this after `abt-language` and before writing a rule.

## 1. There are no modes. Group with `within`.

The first formulation had `+A`/`-A` annotations and the error `'+A' of 'X' is an input and must be bound before the call` cost eight of twelve runs. The second formulation has **no modes on identity arguments**: entity and key arguments may be bound or free at any call. What the modes also did silently, choosing the group of a rank, is now written:

```
rel ranked(A: Equity, @T, K: Count)
ranked(A, T, K) :- bar(T), K = rank(score(A, T, S) by (S desc, A asc)) within (T).
```

`within (T)` ranks every `A` at the bar. `within (A, T)` would rank one tuple and every rank would be 1 (the Haiku failure: the liquidity decile never held). The rule: **the variables that vary inside the reduction are not in `within`; the ones held fixed are.** `rank`, `rank_average` and `top[k]` require `within`; `count`, `sum`, `mean` over a cross-section take it too (`N = count(A) over (long_leg(A, T)) within (T)`).

Bracketed parameters (`sma[N]`, `mean_rows[20, close]`) are inputs and are always bound by the caller; never put a variable there that nothing binds.

## 2. Silent emptiness: a clean check is not a working program

A strategy can check clean and decide nothing. **`decisions = 0` is a failure, never a result.** Every Haiku book did it, and each time one construct left the decision rule empty:

- a wrong label (`member(A, T, "SP500")` when the vocabulary is `"SPX"`): WF-15 now checks label literals against the catalog, but only for the labels you *write*; read `schema("equity")` for the vocabulary;
- a window that can never fill (`window[20d] min 20` over sessions: twenty calendar days hold about fourteen bars; `window[0d] min 20`);
- a join on the wrong key (daily features read at a minute bar without `asof`/the session calendar);
- a base case at the first bar of the whole dataset instead of the entity's first row;
- a guard that excludes everything (`abs(X) > 0` where X is exactly 0 by construction).

How to detect it, every time, before reading any metric:
1. after a `plan_run`, read `runs[].decisions` in the result: every `run` step must have `decisions > 0`;
2. if one has 0, ask the kernel: `query_run(run_id, "coverage over <window>")` prints the first relation on every path to `decide` that is empty, with its `why not` at the first bar; then `query_run(run_id, "why not decide(T, _) at <a bar you expected a trade>")`;
3. fix the rule the report names; do not change the idea to make something fire.

## 3. The previous own row

`prev(T, T0)` steps the **decision calendar**, not the entity's rows. An entity that has no bar at `T0` (a holiday on one exchange, a stock listed later) gets nothing from `close(A, T0, P)`. The first formulation's idiom was `prev` plus an `asof` read of the entity's own relation; the second formulation has it in the library:

```
prev_row(A, T, T1)   # the entity's previous own row (library calendar)
first_row(A, T)      # its first row
row_number(A, T, K)  # 0 at its first row
```

Use `prev_row` for "yesterday's close of this market", for latches and for anything recursive per entity; use `prev` only for the book (`nav`, `position`) and for bar-level relations. **Never put a constant inside an `asof` pattern**: `R(A, T1, 5) asof T0` reads the latest row *with value 5*, which is not what anyone means, and it cost a run twenty minutes.

Library recursions you should not rewrite: `cumsum[R]`, `cumprod[R]` (a product as a recursion, not a sum of logs), `ewm[λ, R]`, `latch[enter, stay]`, `bars_held`, `entry_price`, `high_since_entry`.

## 4. `rows` versus `window`

- `rows[N]` is the entity's last N rows on the rule's calendar and **requires all N** unless you write `min K`. "ATR over the last 20 sessions" is `rows[20]` on the session calendar.
- `window[δ]` is a calendar duration (`20d`, `3mo`, `1y`) and **always needs `min K`**. "The 52-week high" is `max_window[52w, 200, close]` or better `max_rows[250, close]` when the author counted bars.
- `prior_window[δ]` and `prior_rows[N]` exclude the current bar: the breakout's prior high.
- The checker warns (W7) when a calendar window's `min K` exceeds the bars its duration can hold; a `window[0d]` is an error.
- When the author says "20 days", decide whether they meant 20 bars (almost always) and write `rows[20]`.

## 5. Units and typed zeros

Values carry dimensions (F-9): `Price<USD>`, `Quantity<Contracts>`, `Notional<USD>`, `Scalar`, `Count`, `Duration`. The checker rejects `Count × Scalar` and `ATR > 0`:

- a dimensioned comparison needs a typed zero: `V > 0 USD/contract`, `Q > 0 units`, `N > 0 shares`;
- a price ratio is a `Scalar` (`P / P0 - 1`), a price difference a `Price`; `log` and `exp` take a `Scalar` (`log(P / P0)`, never `log(P)`);
- `Price × Quantity = Notional`; `Notional / (Price × Scalar)` is a `Quantity` when the point value multiplies the price: `N = risk / (V * PV)` with `V: Price<USD>` and `PV: Scalar` gives contracts;
- literals: `100 shares`, `2 contracts`, `1_000_000 USD`, `60 USD/share`, `0 USD/contract`, `20d`, `3mo`, `30min`, `0.02`, `"SPX"`;
- **do not remove a guard to silence a type error; type it.**

On continuous futures (house rules §7): signals in points or ATR units, never `%` returns, ratios or logs of a back-adjusted level (it can be negative).

## 6. No disjunction, no conditional expression

There is no `or`, no `;`, no `if … else`, no `not (A, B)`, no `-T`. Write:

- **one rule per alternative** for a disjunction: `aligned(A, T) :- signal(…, G, …), abs(G) < 1.` and `aligned(A, T) :- signal(…, M, G, …), G * M > 0.`;
- a **`case` form** for a conditional value, with complementary guards:
  `Dir = case { abs(M) >= 2 * Q, aligned(A, T) -> sign(M) ; abs(M) < 2 * Q, G <= -0.75 -> -1 }` (no `else` means "no value", never a stuck instance);
- `sign(X)`, `greatest`, `least`, `clamp`, `abs` instead of branches;
- a negation only over a **closed** relation (the universe, the book's `held`/`flat`, a library relation marked closed, a relation whose demand is `complete`); `not above(A, T0)` over a `close`-derived relation without `complete` is rejected (N).

## 7. Order does not matter; guard where you divide

Bodies are conjunctions under Kleene's three-valued logic: a false guard excludes the instance wherever it is written, and a division by zero makes the instance **stuck** only when no guard excluded it. So guard in the rule that divides, and only there: a broad guard in a shared relation (`abs(C30 - O0) > 0` in `signal`) lost 167 legs that never divided.

## 8. Calendars and resolution

Two resolutions never meet "only through `resample`": a relation declares its calendar on the head (`rel atr(A: Future, @S: session, V: Price<USD>)`), a rule at a finer calendar reads a coarser relation **as of** the bar, and the bar views `close@session`, `high@weekly` are derived. For intraday: `clock { decide on base at session + 30min }`, features on `@S: session`, read at the decision bar through `session_of(A, T, S), prev(S, S1), atr(A, S1, V)`: **the previous session's** value, because the current session's own aggregate is not known intraday (WF-6 rejects reading it).

Daily rules on a futures program: `need bars @1d` and `clock { decide on base }` with the base derived from the store's minute bars by the views; you do not write a `resample`.

## 9. The executor's defaults are in the program

Nothing comes from a flag. The house presets `futures_house` and `equities_house` (house rules §15) fix capital (fixed, no compounding), commissions, no slippage, fractional lots, unconstrained leverage, `actions apply`, `delisting last_price`. **Write `execution futures_house` (or `equities_house`)** and nothing else in that block unless you are writing a named single-field variant (`no_costs`, `moo`, `moc`). The harness refuses any other execution block. `report { calendar base   periods 252 per year }` for daily futures; `sessions of "ES"` for intraday.

## 10. Environment vocabulary

Read before writing: `schema("future")` for the primitives, `session`, `point_value`, `commission`, `member` and the lists the store holds; `schema("equity")` for `listed`, `member`'s lists (`"SPX"`, `"SP500"`, …), `delisted` reasons; `catalog("future")` for the roots and their spans; `resolve_symbols(["QQQ", "ES"])` for what the author's symbol is in the store. A label you did not see in the schema is wrong.

## 11. The ATR and the sizing idiom (house rules §3)

```
rel atr(A: Future, @T, V: Price<USD>)
atr(A, T, V) :- universe(A, T), V0 = mean(R) over (T1 in rows[20], true_range(A, T1, R)),
    point_value(A, T, PV), V = greatest(V0, 10 * tick_size)          # tick from the catalog where held; else V = V0
rel n_full(A: Future, @T, N: Quantity<Contracts>)
n_full(A, T, N) :- atr(A, T, V), V > 0 USD/contract, point_value(A, T, PV), capital(T, C),
    N = risk_pct * C / (stop_ix * V * PV).
```

`true_range` is in the `bars` library and uses `prev_row` (the previous own session's close). ATR20 includes the current bar, so it is known at the close and the decision fills at `moc` or at the next `moo`. `N_full` is fixed at the first fill of a trade: latch it (`entry_price`, `latch`) rather than recomputing it on scale-ins.

## 12. Before you call `check`, read your program once for

- every block present (`data`, `uses`, `calendars` when needed, `clock`, `universe`, `params`, `rules`, `decisions`, `execution`, `report`);
- every relation used is declared (`rel`) or from a `uses` module or the catalog;
- every `need` the rules read (`bars`, `actions`, `listing`, `lists "…"`, `sessions`, `point_value`, `commission`);
- `within` on every `rank`/`top`/cross-sectional aggregate;
- typed zeros; `rows` where bars were meant; `prev_row` where the entity's own previous bar was meant;
- a `decide` rule that can fire on the first day the data allows: no window longer than the history, no label you did not see in the schema.

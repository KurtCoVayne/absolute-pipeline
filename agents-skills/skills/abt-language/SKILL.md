---
name: abt-language
version: 1
---
# The abt language, condensed for writing a program

`abt` is a Datalog-like strategy language (`docs/language-v2.md`, the second formulation). A program is **closed**: every input to its meaning is in its text (the data it needs, its universe, its execution model, its reporting), so two runs of one program hash are identical. You write form 1 (the text); the checker (`check`) judges it; the kernel runs it under a plan. There are no flags.

## 1. A strategy is nine blocks

```
strategy NAME {
  data       { class CLASS   need …   require reviewed | allow unreviewed   prefer latest }
  uses       calendar, stat, bars, book          # library modules the rules read
  calendars  { NAME = base | days | weeks(n) | months(n) | years(n) | sessions | sessions of "ROOT" }   # when a rule names one
  clock      { decide on CALENDAR [at session + OFFSET] }
  universe   { [instruments { NAME [from DATE]; … }]   rules for universe(A, @T) }
  params     { NAME : TYPE = LITERAL [in LO..HI]   … }
  rules      { rel …   rules …   [observe R [where …] [trace instances]] }
  decisions  { silence hold | flat   orders ORDER   decide rules }
  execution  futures_house | equities_house [with { FIELD VALUE }]
  report     { calendar CALENDAR   periods N per year   [window DATE .. DATE] }
}
```

A library is `library NAME { data { class any [need bars] }  uses …  rules { … } }`; you will not write one. A missing block or field is an error naming it (P); never a default.

### `data`: the demand

```
data {
  class    future                        # equity | future | series | option
  need     bars @1d from 1990-01-01      # open, high, low, close, volume at @1d or finer, from the date on
  need     sessions, point_value, commission     # futures: the session relation, the multiplier, the per-side commission
  need     actions, listing              # equities: split, dividend, delisted; listed, ticker
  need     lists "SPX"                   # member(A, T, "SPX"); the label must be in the catalog (schema)
  need     series "VIX" @1w              # value("VIX", T, V)
  require  reviewed                      # or: allow unreviewed (warned on every run)
  prefer   latest
}
```

`need bars @1m from … complete` for intraday; `complete` means a missing bar is "did not trade", which lets bar primitives be negated. A rule that reads a primitive the demand did not ask for is an error (E). Every label literal in a rule must occur in a `need` (V).

### `clock` and `calendars`

- Daily: `clock { decide on base }` (the base calendar of the class: `@1d` for equities; the store derives daily bars for futures from the minute base through the bar views).
- Intraday futures: `calendars { session = sessions   es_days = sessions of "ES" }` and `clock { decide on base at session + 30min }`: the bar labelled open + 30 minutes, or the first traded bar after it.
- Weekly or monthly: `calendars { weekly = weeks(1) }`, `clock { decide on weekly }`; `prev`, `rows` and windows then step a week.

### `universe`: the entity domain

```
universe { universe(A, T) :- listed(A, T), traded@base(A, T). }                       # every listed equity with a bar
universe { universe(A, T) :- listed(A, T), member(A, T, "SPX"), traded@base(A, T). }   # index members as of the bar
universe { universe(A, T) :- member(A, T, "HOME-26"), session_of(A, T, _). }           # a futures list the store holds
universe {
  instruments { ES from 2000-01-01;  NQ from 2000-01-01;  CL from 2007-01-01;  GC }   # an explicit table: counted as degrees of freedom and warned (W9)
  universe(A, T) :- instruments(A, From), traded@base(A, T), T >= From.
}
```

`universe(A, @T)` is complete by definition, so `prev_row`, `first_row`, `bar` and `flat` may be negated. The house futures universe is every root the store holds (house rules §2): `catalog("future")` lists them; `schema("future")` lists the `member` lists.

### `params`

```
params {
  lookback : Duration = 3mo in 1mo..12mo
  n        : Count = 20 in 10..60
  entry    : Scalar = 2.0 in 1.0..3.0
  risk_pct : Scalar = 0.0075
  stop_ix  : Scalar = 2
  qty      : Quantity<Contracts> = 1 contracts
  floor    : Notional<USD> = 1_000_000 USD
  side     : Scalar = 0            # 0 both, 1 long only, -1 short only: a variant a plan's `run … at { side = 1 }` selects
  y        : Future = "ES"         # an instrument parameter
}
```

Types: `Scalar`, `Count`, `Duration`, `Price<USD>`, `Quantity<Contracts|Shares>`, `Notional<USD>`, `Timestamp`, `Equity`, `Future`, `Label`. A range `in LO..HI` is what a plan's `grid` may vary. A number inside a rule that is a choice belongs in `params` (W5 otherwise); structure (`-1`, `1/3`, `2`) may be a literal.

### `rules`

Every derived relation is declared then defined:

```
rel sma[N: Count](A: Future, @T, M: Price<USD>)                 # bracketed parameters are inputs, always bound by the caller
sma[N](A, T, M) :- universe(A, T), M = mean(P) over (T1 in rows[N], close(A, T1, P)).

rel above(A: Future, @T)
above(A, T) :- sma[fast](A, T, F), sma[slow](A, T, S), F > S.

rel atr(A: Future, @S: session, V: Price<USD>)                   # a relation on another calendar names it on the head
atr(A, S, V) :- session(A, S, _), V = mean(R) over (S1 in rows[20], true_range@session(A, S1, R)).
```

- Signature: entity arguments (`A: Future`), the temporal key (`@T`, or `@S: session`), value arguments. No modes.
- A body is a conjunction of atoms, negated atoms (`not held(A, T)`), comparisons, assignments (`X = …`), aggregations and reductions. Order is irrelevant; a false guard excludes the instance wherever it stands.
- Temporal builtins: `prev(T, T0)`, `lag(T, 3mo, T0)`, `month_start(T)`, `day_start(T)`, `session_of(A, T, S)`, `bar(T)`; `R(A, T1, X) asof T0` reads the latest row at or before `T0`.
- Bar views: `close@session(A, S, P)`, `high@weekly`, `traded@base(A, T)`, `true_range@session`.
- Aggregation: `V = AGG(expr) over (T1 in RANGE, atom(A, T1, X), …)` with `AGG` ∈ `count, sum, mean, std, min, max, median, first, last, ols_slope(Y, X)`, and `RANGE` ∈ `rows[N] [min K]`, `prior_rows[N] [min K]`, `window[δ] min K`, `prior_window[δ] min K`, `bucket(C, T)`, `session(X, [a, b))`.
- Cross-section at a bar: `K = rank(R(A, T, S) by (S desc, A asc)) within (T)`, `top[n](R(A, T, S) by (S desc, A asc)) within (T)`, `N = count(A) over (R(A, T)) within (T)`. `within` names the group; variables outside it are fresh.
- Functions: `abs, least, greatest, log, exp, sqrt, sign, floor, ceil, clamp(x, lo, hi), decile(rank, n)`.
- `case`: `X = case { G1 -> e1 ; G2 -> e2 ; else -> e0 }` with closed, complementary guards.
- `observe R [where A = "ES"] [trace instances]` materialises `R` at every bar for the ledger without changing a decision.

### `decisions`

```
decisions {
  silence hold                 # an instrument with no decision keeps its position; `silence flat` closes it
  orders  moo_moc              # the order of a decision that names none: market | moo | moc | moo_moc
  decide(T, trade(A, N))  :- enter_long(A, T), flat(A, T), n_full(A, T, N).           # a signed delta
  decide(T, trade(A, Q))  :- exit(A, T), held(A, T, Q0), Q = -Q0.                       # close: minus what is held
  decide(T, hold(A, W))   :- long_leg(A, T), n_long(T, N), W = 0.4 / N.                 # a target weight of capital
  decide(T, hold(A, 0))   :- bar(T), month_start(T), in_book(A, T), not long_leg(A, T).
  decide(T, hold(A, N))   :- leg(A, T, Dir, V), point_value(A, T, PV), N = Dir * risk / (V * PV).   # a target quantity
  decide(T, trade(A, N) stop(P)) :- …                                                   # an order on the decision: market | moo | moc | moo_moc | limit(P[, TIF]) | stop(P[, TIF])
}
```

One decision per instrument per bar (two conflict at runtime). A decision at bar `i` uses data at or before `i`'s close and fills at that close (`moc`) or the next open (`moo`) (house rules §5). A stop order placed at the decision fills intrabar at `min(open, stop)` for longs.

Kernel relations the rules may read at the decision calendar: `position(A, @T, Q)`, `cash(@T, C)`, `nav(@T, N)`, `capital(@T, C)` (the fixed capital under the house presets), `mark(A, @T, P)`, `fill(A, @T, Q, P)`, `decided(@T0, D)`. The `book` library wraps them: `held`, `short`, `in_book`, `flat`, `weight_now`, `exposure`, `nav_return`, `drawdown`, `bought_within[δ]`.

### `execution` and `report`

`execution futures_house` or `execution equities_house` (house rules §15), verbatim; a variant changes one field in a program named for it: `execution futures_house with { orders moc }` in `strategy <slug>_moc`. The fields exist so that `show(program, "execution")` prints them; you never write them out.

`report { calendar base   periods 252 per year }` daily; `report { calendar es_days   periods 252 per year }` intraday on `sessions of "ES"`; `report { calendar weekly   periods 52 per year }` weekly. A `window` is the plan's, not the program's.

## 2. The standard library (`uses`)

| Module | Relations |
| --- | --- |
| `calendar` | `bar(@T)`, `prev_row(A, @T, T1)`, `first_row(A, @T)`, `row_number(A, @T, K)` |
| `catalog` | `split_factor`, `cum_split`, `adj_close`, `dividend_at`, `dividend_or_zero`, `total_return`, `log_return`, `tr_index` (equities, actions applied causally) |
| `stat` | `mean_rows[N, R]`, `sum_rows`, `std_rows`, `max_rows`, `min_rows`, `median_rows[N, K, R]`, `mean_window[δ, K, R]`, `sum_window`, `std_window`, `max_window`, `min_window`, `max_prior_window`, `mean_prior_window`, `zscore_rows[N, R]`, `zscore_window[δ, K, R]`, `cumsum[R]`, `cumprod[R]`, `ewm[λ, R]` |
| `bars` | `true_range(A, @T, V)`, `range`, `gap_points`, `gap`, `price_return`, `log_price_return`, `dollar_volume`, `adv[N]`, `liquid[Floor]` |
| `xsec` | `decile[R]`, `zscore_xs[R]`, `share_above[R, Th]` |
| `state` | `latch[enter, stay](A, @T)`, `bars_held(A, @T, N)`, `entry_price(A, @T, P)`, `high_since_entry(A, @T, H)` |
| `book` | `held(A, @T, Q)`, `short`, `in_book`, `flat`, `weight_now`, `exposure`, `nav_return`, `peak_nav`, `drawdown`, `nav_vol[N]`, `bought_within[δ]` |

A relation parameter is written by name: `mean_rows[20, close](A, T, M)`, `latch[enter_sig, stay_sig](A, T)`, `std_rows[20, log_price_return]`. There is nothing in the library that is a trading idea: `momentum`, `sma` as a trend and the like are rules you write.

## 3. The catalogs (what `schema(class)` prints)

**`future`** (base `@1m`, unit `contracts`): `open, high, low, close, volume` per root as **back-adjusted continuous** series whose prices may cross zero; `session(A, @S, Open)` complete, one per root and session labelled at the session's close; `point_value(A, @T, V: Scalar)` complete (currency per point per contract); `commission(A, @T, C: Notional<USD>)` complete (per contract per side); `member(A, @T, List: Label)` complete; `usable(A, @T)` complete; `settlement(A, @T, P)`; `open_interest(A, @T, N)`; `margin(A, @T, Initial, Maintenance)` complete. Lists: as the store declares them (`schema("future")` prints them; the corpus's example declares `"HOME-26"`, `"HOLD-6"`).

**`equity`** (base `@1d`, unit `shares`): `open, high, low, close, volume` as traded; `listed(A, @T)` complete; `ticker(A, @T, S: Label)`; `split(A, @T, Factor)`; `dividend(A, @T, Ex, Pay, Amount)`; `special_dividend`; `delisted(A, @T, Reason: Label)`; `member(A, @T, List: Label)`; `classification(A, @T, Scheme, Code)`; `margin`. Lists `"SPX", "SP500", "SP400", "SP600", "ETF"`; reasons `"bankruptcy", "acquisition", "voluntary", "regulatory"`.

**`series`**: `value(Name: Label, @T, V: Scalar)` with names such as `"SPXTR" @1d`, `"VIX" @1w`.

Read the real catalog of the store before writing (`schema`, `catalog`): lists, roots and spans differ from the corpus's example.

## 4. A complete daily futures program on the house preset

```
# Buy a 50-day Donchian breakout at the next open, exit on a 3-ATR trailing stop; house sizing.
strategy donchian_50 {
  data {
    class    future
    need     bars @1d from 2000-01-01
    need     sessions, point_value, commission
    require  reviewed
    prefer   latest
  }
  uses  calendar, stat, bars, state, book
  clock { decide on base }
  universe {
    universe(A, T) :- member(A, T, "HOUSE"), traded@base(A, T).
  }
  params {
    entry_rows : Count = 50 in 20..100
    trail_atr  : Scalar = 3 in 1..5
    atr_rows   : Count = 20
    risk_pct   : Scalar = 0.0075
    stop_ix    : Scalar = 2
  }
  rules {
    rel atr(A: Future, @T, V: Price<USD>)
    atr(A, T, V) :- universe(A, T), V = mean(R) over (T1 in rows[atr_rows], true_range(A, T1, R)).

    rel n_full(A: Future, @T, N: Quantity<Contracts>)
    n_full(A, T, N) :- atr(A, T, V), V > 0 USD/contract, point_value(A, T, PV), capital(T, C),
        N = risk_pct * C / (stop_ix * V * PV).

    rel breakout(A: Future, @T)
    breakout(A, T) :- close(A, T, P), max_prior_window[entry_rows, entry_rows, close](A, T, H), P > H.

    rel trail(A: Future, @T, S: Price<USD>)
    trail(A, T, S) :- held(A, T, _), high_since_entry(A, T, H), atr(A, T, V), S = H - trail_atr * V.
    rel stopped(A: Future, @T)
    stopped(A, T) :- trail(A, T, S), close(A, T, P), P < S.
  }
  decisions {
    silence hold
    orders  moo
    decide(T, trade(A, N)) :- breakout(A, T), flat(A, T), n_full(A, T, N).
    decide(T, trade(A, Q)) :- stopped(A, T), held(A, T, Q0), Q = -Q0.
  }
  execution futures_house
  report { calendar base   periods 252 per year }
}
```

(`"HOUSE"` stands for whatever list the store's `schema("future")` names for the house universe; use that label or an `instruments` table from `catalog("future")`.) `max_prior_window[entry_rows, entry_rows, close]` takes a Duration in the library's signature; for a row count use `H = max(P1) over (T1 in prior_rows[entry_rows], close(A, T1, P1))` inline.

## 5. The checker's codes

Errors: `U` undeclared or reserved name; `E` a primitive outside the demand; `B` unbound head variable; `M` unbound input (the diagnostic names the variable and the literal that needs it); `T` types (names the typed zero to write); `R` recursion without a temporal step; `N` negation over an open relation; `F` causality (reads the future: a bucket before its close, a session's own aggregate intraday); `D` determinism; `S` stratification; `Z` no `decide` rule; `X` calendars (a relation named at an undeclared or incomparable calendar); `K` keyedness (a temporal recursion must be keyed: complementary guards or a `case`); `G` grouping (`within` missing or a variable outside it not fresh); `P` closed configuration (a block or field missing); `V` vocabulary (a label or `instruments` name the catalog lacks); `Q` demand (the `data` block has no answer); `O` observation. Warnings: `W5` a structural constant that should be a parameter; `W6` a ticker literal; `W7` a calendar window whose `min K` cannot fill; `W8` on run: a relation on the path to `decide` never fired; `W9` an `instruments` table (a hindsight constant, counted). A clean check has no error; warnings are read and either fixed or explained in the notes.

## 6. Queries you may run through `query_run(run_id, text)` after a run

`? R(A, T, X) at T = 2020-03-06`, `? R(A, T, X) for A = "ES" over 2020-01 .. 2020-03`, `? decide(T, D) at T`, `why R(a, t, x)`, `why not R(a, t)`, `trace decide at T`, `coverage over WINDOW`, `show program`, `show plan R`, `show calendars`, `ledger R` (the whole extension of `R` as a table). The `read-results` skill says how to read them.

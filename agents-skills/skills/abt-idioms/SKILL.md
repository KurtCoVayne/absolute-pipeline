---
name: abt-idioms
version: 3
---
# abt idioms and pitfalls

The forms that work on this store, each checked clean with `abt check` on it (Oct 19, 2026); the run status of each is in its heading. And the mistakes small models make. Read after `abt-language`.

## 1. Daily futures: decide on each root's sessions (checked and run: ES, 2018 to 2026, decides)

A daily futures program decides at each root's session close, on the session calendar, and reads the bars through their `@session` views (WF-16 refuses `close(A, T, P)` keyed on a session):

```
data      { class future   need bars @1m from <window start - warm-up> complete   need sessions, point_value   require reviewed   prefer latest }
uses      calendar, bars, book, state, stat
calendars { session = sessions }
clock     { decide on session }
universe  { instruments { ES from <window start> }
            universe(A, T) :- instruments(A, From), session(A, T, _), T >= From. }
rules     { rel atr(A: Future, @T, V: Price<USD>)
            atr(A, T, V) :- universe(A, T), V = mean(R) over (T1 in rows[atr_rows], true_range@session(A, T1, R)).
            rel size(A: Future, @T, N: Quantity<Contracts>)
            size(A, T, N) :- atr(A, T, V), V > 0 USD/contract, point_value(A, T, PV), PV > 0, N = risk / (stop_ix * V * PV). }
decisions { silence hold   orders moo
            decide(T, hold(A, N)) :- <entry>(A, T), flat(A, T), size(A, T, N).
            decide(T, hold(A, 0 contracts)) :- held(A, T, _), <exit>(A, T). }
execution futures_house with { orders moo }          # in a strategy named <slug>_moo: moo_moc is intraday (WF-14)
report    { calendar session   periods 252 per year }
```

- **Memory and time:** a run streams the minute bars from the `need`'s `from` date. Put `from` at the window's start less the warm-up the longest window needs, never years earlier. Name the roots in `instruments`.
- `PV > 0`: a root's point value can be 0 in the store; without the guard the run halts on a division by zero.
- Sizing (house rules §3): `risk = 7500 USD`, `stop_ix = 2`; `hold(A, N)` is a target in contracts; `hold(A, 0 contracts)` closes.

## 2. Intraday: the session so far at a decision minute (checked and run: ES, July to August 2026, decides)

"Three green 5-minute bars in the first 30 minutes": decide 30 minutes after each session's open, reading the session so far as cumulative intervals `[0min, M)` (an interval starting after `0min`, `session(X, [5min, 10min))`, panics the kernel today: use `[0min, M)` only):

```
calendars { session = sessions   es_days = sessions of "ES" }
clock     { decide on base at session + 30min }
universe  { instruments { ES from <date> }   universe(A, T) :- instruments(A, From), session_of(A, T, _), T >= From. }
rules {
  rel atr(A: Future, @S: session, V: Price<USD>)
  atr(A, S, V) :- session(A, S, _), V = mean(R) over (S1 in rows[atr_rows], true_range@session(A, S1, R)).
  rel open0(A: Future, @X: base | session, O: Price<USD>)
  open0(A, X, O) :- O = first(P) over (T1 in session(X, [0min, 1min)), open(A, T1, P)).
  rel close_at[M: Duration](A: Future, @X: base | session, C: Price<USD>)
  close_at[M](A, X, C) :- C = last(P) over (T1 in session(X, [0min, M)), close(A, T1, P)).
  rel three_green(A: Future, @T, V: Price<USD>)     # each 5-minute close above the one before, the first above the open
  three_green(A, T, V) :- universe(A, T), open0(A, T, O), close_at[5min](A, T, C1), close_at[10min](A, T, C2), close_at[15min](A, T, C3),
      C1 > O, C2 > C1, C3 > C2, session_of(A, T, S), prev(S, S1), atr(A, S1, V), V > 0 USD/contract.   # the PREVIOUS session's ATR (WF-6)
}
decisions { silence flat   orders moo_moc   decide(T, hold(A, N)) :- three_green(A, T, V), point_value(A, T, PV), PV > 0, N = risk / (stop_ix * V * PV). }
execution futures_house          # moo_moc on an intraday clock: enter at the next print, flat at the session's close
report    { calendar es_days   periods 252 per year }
```

A 5-minute bar's green test is its close above the previous 5-minute close (its open, up to the gap between prints): an approximation, stated as one. "A trend day up" then is the session's close above its open by k ATRs: `close@session(A, S, C), open@session …`; write the assumption.

## 3. Swings, Fibonacci and a mechanical Elliott wave 2 (checked; pivots and swings verified on BTC, no wave-2 entry at impulse 4 over a year)

Pivots are confirmed k sessions late (causal); the last swing is an `asof` read of the pivot relation, which binds when it happened:

```
params { k : Count = 3 in 2..5   impulse : Scalar = 4 in 2..8   lo_fib : Scalar = 0.5   hi_fib : Scalar = 0.786 … }
rel pivot_high(A: Future, @T, H: Price<USD>)        # the bar k back is the strict high of the last 2k+1
pivot_high(A, T, H) :- universe(A, T), H = max(P) over (T1 in rows[2 * k + 1], high@session(A, T1, P)),
    H1 = max(P) over (T1 in rows[k + 1], high@session(A, T1, P)), H2 = max(P) over (T1 in rows[k], high@session(A, T1, P)), H1 = H, H2 < H.
rel swing_high(A: Future, @T, H: Price<USD>, At: Timestamp)
swing_high(A, T, H, At) :- universe(A, T), pivot_high(A, At, H) asof T.
# pivot_low / swing_low: the same with low@session, min, L1 = L, L2 > L.
rel wave2_buy(A: Future, @T, V: Price<USD>)          # assumption: wave 1 = a low-to-high swing of >= impulse ATRs;
wave2_buy(A, T, V) :- swing_high(A, T, H, TH), swing_low(A, T, L, TL), TL < TH, atr(A, T, V), V > 0 USD/contract,
    (H - L) / V >= impulse, close@session(A, T, P), P <= H - lo_fib * (H - L), P >= H - hi_fib * (H - L).   # wave 2 = a 0.5-0.786 retracement
```

Exits: the wave-1 low broken (`swing_low(A, T, L, _), close@session(A, T, P), P < L`) or a new pivot high. A Fibonacci level is `H - f * (H - L)` (up swing) or `L + f * (H - L)` (down swing). Elliott counts are not unique: always state the mechanical reading as an assumption and test it; it is never a reason for `not_testable`.

## 4. Calendar rules: month boundaries, the Nth-last session, turn of month (checked and run: ES, 2026, decides monthly)

**`month_start(T)` is never true on a `sessions` clock**: a program that counts from it decides nothing. Read month ends from a monthly calendar instead:

```
calendars { session = sessions   monthly = months(1) }
rel mend(A: Future, @M: monthly)                         # each month's last bar
mend(A, M) :- traded@monthly(A, M).
rel first_of_month(A: Future, @T)                        # a month end fell after the previous session
first_of_month(A, T) :- universe(A, T), prev_row(A, T, T0), mend(A, M) asof T, M < T, mend(A, M0) asof T0, M > M0.
first_of_month(A, T) :- universe(A, T), prev_row(A, T, T0), mend(A, M0) asof T0, M0 = T0.
rel msess(A: Future, @T, K: Count)                       # the session's number in its month, 1 at the first
msess(A, T, 1) :- first_of_month(A, T).
msess(A, T, K) :- universe(A, T), not first_of_month(A, T), prev_row(A, T, T0), msess(A, T0, K0), K = K0 + 1.
rel mlen_prev(A: Future, @T, N: Count)                   # last month's session count, carried through this month
mlen_prev(A, T, N) :- first_of_month(A, T), prev_row(A, T, T0), msess(A, T0, N).
mlen_prev(A, T, N) :- universe(A, T), not first_of_month(A, T), prev_row(A, T, T0), mlen_prev(A, T0, N).
rel nth_last(A: Future, @T)                              # ASSUMPTION: this month has as many sessions as the last (±1-2)
nth_last(A, T) :- msess(A, T, K), mlen_prev(A, T, N), K = N - nth + 1.
```

The store has no forward exchange calendar, so "the Nth-last session" is this labelled approximation; turn of month is `first_of_month` (or `nth_last` with `nth = 1` for the last session, approximated); hold N sessions with `bars_held(A, T, B), B >= hold_n` (`uses state`). Day-of-week has no builtin: approximate a week's first session as the first after a gap of 2 days or more and say so.

**One root of several** (a check or a sanity run): an instrument parameter compared with the entity, checked and run:

```
params   { only : Future = "ES" }
universe { instruments { ES from <date>;  NQ from <date> }
           universe(A, T) :- instruments(A, From), session(A, T, _), T >= From, A = only. }
```

## 5. Weekly averages on daily bars (checked)

```
calendars { session = sessions   weekly = weeks(1) }
rel wclose(A: Future, @W: weekly, P: Price<USD>)
wclose(A, W, P) :- close@weekly(A, W, P).
rel wema(A: Future, @W: weekly, E: Price<USD>)
wema(A, W, E) :- ewm[ema_l, wclose](A, W, E).             # ema_l = 2 / (n + 1) as a param: a bracket takes a name or a number
rel wnum(A: Future, @W: weekly, K: Count)
wnum(A, W, K) :- wclose(A, W, _), row_number(A, W, K).
rel wwma(A: Future, @W: weekly, M: Price<USD>)           # linear weights 1..n as Scalars (a Count becomes a Scalar only by division)
wwma(A, W, M) :- wnum(A, W, K), SW = sum(P * ((J - K + n) / n)) over (W1 in rows[n], wclose(A, W1, P), wnum(A, W1, J)),
    M = SW / ((n + 1) / 2).
# read at a session: wwma(A, W, M) asof T
```

A 200-week average needs four years of warm-up: start the data four years before the window (BTC starts 2017-12-18, so its first four years are warm-up).

## 6. Actual prices, spreads and ratios

House rules §7: levels, spreads and ratios on actual prices, P&L on the back-adjusted continuous roots. A back-adjusted series' old levels drift far from the real prices (HO's continuous close was negative in 2013), so a rule that reads a level, a ratio, a percentage or a log on it is wrong there; differences and P&L are right.

**Today:** the Databento roots are back-adjusted only, so a rule on a level, a ratio or a percentage reads a drifted price: state it on differences (a spread's change, a z-score of changes) and label it; the held contract's own price (an `adjustment` relation) is being added.

**Not working today (an engine defect, reported Oct 20):** the form below, through the kernel's `contract(C, T, A, Expiry, PointValue)`, returns no rows on either store; don't use it until it's fixed.

```
rel actual_close(A: Future, @T, P: Price<USD>)       # the held contract's own close, on the root's session
actual_close(A, T, P) :- universe(A, T), contract(C, T, A, _, _), P = last(P1) over (T1 in bucket(session, T), close(C, T1, P1)).
```

A 3:2:1 crack spread in USD per barrel (CL quotes USD/bbl; RB and HO USD/gal, 42 gal a barrel), traded on the continuous roots:

```
params { cl : Future = "CL"   rb : Future = "RB"   ho : Future = "HO"   w_cl : Scalar = 3   w_rb : Scalar = 2   w_ho : Scalar = 1   gal_bbl : Scalar = 42 … }
rel crack(@T, S: Price<USD>)
crack(T, S) :- actual_close(cl, T, Pc), actual_close(rb, T, Pr), actual_close(ho, T, Ph), S = (w_rb * gal_bbl * Pr + w_ho * gal_bbl * Ph - w_cl * Pc) / w_cl.
rel crack_z(@T, Z: Scalar)
crack_z(T, Z) :- crack(T, S), M = mean(X) over (T1 in rows[z_rows], crack(T1, X)), D = std(X) over (T1 in rows[z_rows], crack(T1, X)),
    D > 0 USD/contract, Z = (S - M) / D.
decide(T, hold(cl, Q)) :- crack_z(T, Z), Z > z_entry, Q = w_cl * combos * 1 contracts.     # long crude, short the products:
decide(T, hold(rb, Q)) :- crack_z(T, Z), Z > z_entry, Q = -w_rb * combos * 1 contracts.    # selling a wide crack
decide(T, hold(ho, Q)) :- crack_z(T, Z), Z > z_entry, Q = -w_ho * combos * 1 contracts.
```

Contract sizes (CL 1,000 bbl, RB and HO 42,000 gal) make 3:2:1 in contracts 3:2:1 in barrels. Roll yield is in the continuous P&L; the spread's level is on the actual contracts. "Actual contracts" is never a reason for `not_testable`.

## 7. Pitfalls that cost runs

- **`decisions = 0` is a failure.** After every `plan_run` read `runs[].decisions`; on 0, `query_run(run_id, "coverage over <window>")` names the empty relation. A wrong label, a window that never fills, a base case at the dataset's first bar.
- **Grouping**: `rank`, `top[k]` and cross-sectional `count/sum/mean` take `within (T)`: the variables held fixed; the ones that vary are not in it.
- **Previous own row**: `prev_row(A, T, T1)` (library `calendar`), not `prev(T, T0)`, for "this market's previous bar". Never a constant inside an `asof` pattern.
- **`rows[N]` vs `window[δ] min K`**: "20 days" means 20 bars, `rows[20]`; `prior_rows` excludes the current bar (breakouts).
- **Units**: typed zeros (`0 USD/contract`, `0 contracts`, `0 units`); `Count × Price` is refused (divide first); `log` takes a Scalar. Type a guard, never delete it.
- **No `or`, `;`, `if`**: one rule per alternative; `case { G1 -> e1 ; G2 -> e2 }`; `sign`, `greatest`, `least`, `clamp`.
- **Negation** only over a closed relation (the universe, `held`/`flat`, a builtin-derived relation like `mstart`, primitives under `complete`). A relation derived through an aggregate is not closed: use an `asof` read instead of `not R(…)`.
- **Bracket arguments** are names or numbers: `ewm[ema_l, wclose]`, not `ewm[2 / (n + 1), wclose]`.
- **Guard where you divide**, not in a shared relation.
- **Literal choices** belong in `params` (W5); the `instruments` table is warned (W9) and fine.

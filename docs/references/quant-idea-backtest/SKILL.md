---
name: quant-idea-backtest
description: Backtest a trading idea found on X/Twitter (or any link, screenshot, or text) on Jacob's 49-futures universe (CSI daily back-adjusted data in D:\claude) the way a careful quant analyst would - fixed-$ ATR sizing with no compounding, MATLAB-style commissions and slippage, 80/20 in-sample/out-of-sample split, equity + drawdown chart, full performance table. Use this skill whenever the user shares an x.com / twitter.com link to a strategy, says "testea esta idea", "backtest this", "pruébalo en futuros", pastes strategy rules, or asks to validate a published system, even if they don't say the word backtest.
---

# Quant idea backtest (futures)

Turn a published idea into a correct, honest backtest on the futures universe. The engine, costs,
sizing and report are already built and verified - **do not rewrite them**. Your work is
(1) extracting the rules faithfully, (2) deciding whether the data can test them, (3) writing a small
Strategy class, (4) running the protocol, (5) interpreting the result like a sceptical quant.
Talk to the user in Spanish (his language); code and file names stay in English.

## 0. Locations

| What | Where |
|---|---|
| Data (CSI `*0@EAB.txt`, one per market) | `D:\claude` on Jacob's PC (Claude Code / Cowork). In a sandbox: ask for a zip of the `*0@EAB.txt` files. |
| Universe + contract specs | `assets/universe.csv` (49 futures, BPV, tick, group, CSI file code) |
| Engine | `scripts/qbt/` (`data.py`, `engine.py`, `metrics.py`, `report.py`) |
| Runner | `scripts/run_backtest.py` |
| Tweet fetcher | `scripts/fetch_tweet.py` (x.com blocks fetches; this uses the fxtwitter mirror) |
| S&P 500 stocks (current & past, Norgate) | `scripts/export_norgate_sp500.py` (run on the PC with NDU open -> `D:\claude\sp500\*.parquet`), `scripts/qbt/equities.py` (panel + share-based simulator: membership filter, delistings, dividends, per-share commissions), `scripts/run_equities.py`, strategies `strategies/eq_*.py` (event-driven `on_close`) |
| Pairs / spread ideas | `scripts/pairs_backtest.py` (two legs, z-score of log spread; needs the UNADJUSTED `<code>03X1N.txt` files for the signal, Panama files for P&L; prints Engle-Granger/ADF diagnostics) |
| Example strategy / template | `scripts/strategies/zscore_scalein_mr.py`, `scripts/strategies/template.py` |
| Conventions in detail | `references/conventions.md` (data, sizing, costs, metrics - read before touching the engine) |
| Quant checklist | `references/protocol.md` (read before writing the conclusion) |

## 1. Get the idea

```
python scripts/fetch_tweet.py <x.com url> --out work/<slug>/tweet
```
Read `tweet.txt`, then **view every downloaded image** - the chart usually carries instrument,
dates, starting capital, net profit, max DD, Sharpe, number of trades. Those are your sanity benchmark.
If the post links to an article, try `web_fetch`; if it is paywalled/registration-only, ask the user to
paste the rules. If the post is a screenshot of rules, read it with the view tool.

Extract into a short spec (keep it, it goes into the report):
instrument(s) and timeframe · entry rule(s) · exit rule(s) · execution timing (same close? next open?) ·
sizing/risk the author used · parameters and their values · what the author reported (capital, profit,
DD, Sharpe, trades, dates) · anything **not stated** that you must assume.

**Never invent a missing rule.** If a rule is ambiguous (which MA? stop or no stop? long only?) state
the assumption explicitly and, when it changes the result materially, run both variants.

## 2. Can our data test it?

Our data: daily OHLC, volume, OI, Panama back-adjusted continuous futures from 1979. Decide:

| Idea needs | Verdict |
|---|---|
| Daily closes / OHLC / ATR / volume / OI | Testable |
| Intraday bars, session open breakouts, time-of-day, "exit at end of day" with intraday stops | **Not testable correctly**. Say so. Offer at most a daily-bar approximation with conservative assumptions, labelled as such, or stop. |
| Options, implied vol, breadth, fundamentals, earnings dates, other tickers | Not available - say so |
| Single stock / ETF (QQQ, SPY, NU) | Testable on the matching future (NQ, ES) and, if the logic is generic, on the whole universe; say which |
| Ratios / % returns on prices | Rewrite in points or ATR units: back-adjusted series can be negative far back (CL April 2020, bonds/currencies in the 80s) |

## 3. Translate the rules into a Strategy

Copy `scripts/strategies/template.py` to `scripts/strategies/<slug>.py` and implement `target(df)`:
a Series of the desired position as a fraction of the full size in [-1, 1], decided at each bar's close.

Pairs, ratios, spreads between two markets: use `pairs_backtest.py`, never the single-market engine, and never compute ratios on Panama series (see conventions.md).

Rules that keep translations correct:
- Signals use data up to and including bar *i*'s close; `exec_at="close"` fills at that close
  (the author's MOC/LOC), `exec_at="next_open"` fills next morning with range-bound slippage.
  Default to the author's timing; run the other as a robustness variant.
- Compute indicators on `df[df.traded]` only, then `.reindex(df.index)`; forward-filled holidays
  must not create duplicate closes.
- Scale-ins: 1/3, 2/3, 1. `N_full` is fixed at the first fill of a trade.
- Initial intrabar stop: set `stop_atr = k`; the engine fills it at `min(open, stop)` with slippage.
  Trailing stops / profit targets: implement inside `target()` on closes, or ask before approximating.
- Long-only ideas stay long-only. Add the mirrored short version only as a labelled diagnostic.
- Keep the author's parameters as the base case. No tuning before the sensitivity step.

Before running on everything, verify the translation on ONE market by printing a few trades next
to the raw closes/indicators (see the ES check in `references/protocol.md`). A translation bug
produces a clean, wrong report.

## 4. Run the protocol

```
python scripts/run_backtest.py \
  --strategy strategies.<slug>:<Class> --params '{...author params...}' \
  --data D:/claude --out work/<slug>/base \
  --title "..." --source "<url>" --rules "<html of rules as tested>" \
  --notes '["assumption 1", "caveat 2"]' \
  --sensitivity '{"p1":[...],"p2":[...]}'
```
Defaults = Jacob's setup: capital 1,000,000 · risk 0.75% = $7,500 per trade, **no compounding** ·
ATR(20), stop index 2 · fractional contracts · $7.5/contract round-turn and per roll · slippage 10% of
remaining bar range on open/stop fills, 0 ticks at the close · min ATR 10 ticks · OOS = last 20% of the
calendar span (cut date printed and shaded on the chart). Change these only if the user asks.

Required runs:
1. **Base** - author's rules and parameters, whole universe (or the subset the idea is meant for).
2. **Sanity** - the author's own instrument (`--symbols NQ` for QQQ, `ES` for SPY ...) over the
   author's date range (`--start/--end`). Compare shape of equity, DD, trade count and win rate with
   the author's chart. Large gaps mean a translation error or a hidden rule - investigate before anything else.
3. **Sensitivity** - 2-D grid of the two most important parameters, in-sample only (the runner never
   touches OOS for the grid). You want a plateau, not a spike.
4. **Variants** that a reviewer will ask about: `--long_only`/`--short_only`, `exec_at` flipped,
   `--groups G2` vs the rest, with/without the stop. Keep them short; put them in a small table.

Report OOS once, with the base parameters. Never pick parameters by looking at OOS.

## 5. Deliver

`report.html` (equity + drawdown with OOS shaded, performance table Full / IS / OOS, trade stats,
time-window table, monthly table, histogram, contribution by market, sensitivity grid) plus the PNGs
and CSVs (`daily_equity.csv`, `trades.csv`, `monthly_returns.csv`). Present the report with
`present_files` (or copy it to a folder the user named) and write the conclusion in chat, in Spanish:

1. What was tested, in one paragraph, including every assumption.
2. Numbers that matter: Avg return % p.a., MaxDD % of capital, MAR, Sharpe, trades, profit factor -
   Full, IS and OOS side by side. OOS verdict in one sentence.
3. Where the P&L comes from (markets, years, long vs short) and whether one market or one year carries it.
4. Sensitivity verdict: plateau or cliff.
5. Honest conclusion: does the idea survive costs, OOS and sensitivity on futures? What would
   be needed to trade it (data, execution type, risk overlay)? Include the author's numbers vs ours.

Red flags to call out explicitly (see `references/protocol.md`): fewer than ~200 trades,
one market > 40% of profit, profits concentrated before 1990 (inflation-era trends), DD duration in
years, PF < 1.2 after costs, Sharpe that collapses OOS, parameter cliffs, intraday logic forced onto daily bars.

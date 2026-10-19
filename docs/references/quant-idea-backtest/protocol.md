# Quant protocol

## A. Translation check (do this before the full run)
Pick one liquid market and print, for 2-3 trades, the raw closes, the indicator values and the fills:
```python
import sys; sys.path.insert(0, "scripts")
from qbt import load_universe, load_markets, Config, run_market
from strategies.<slug> import <Class>
uni = load_universe("assets/universe.csv"); m = load_markets("D:/claude", uni, ["ES"], 20)["ES"]
d, trades = run_market(m, <Class>(), Config())
t = trades[-1]; print(t.row()); print(m.df.loc[t.entry_date - pd.Timedelta(days=10): t.exit_date, ["open","high","low","close","atr"]])
```
Recompute the trade P&L by hand: sum(contracts_k x (exit - entry_k)) x BPV - commission. If it does not
match to the cent, fix the strategy (or report a bug) before running anything else.
Also confirm: no fill on a `traded=False` day, scale-in uses the same `N_full`, stop fills at
`min(open, stop)` for longs.

## B. Lookahead audit
- Does `target(i)` use `close[i]`? Then execution must be `close` (MOC/LOC) or `next_open`. Never
  "today's open" with a signal from today's close.
- Rolling windows: `rolling(n)` on traded bars only. `ewm` likewise.
- Nothing from `df` beyond row i (no `shift(-1)`, no full-sample normalisation, no z-scores of the whole
  history, no future-based rankings).
- Parameters chosen on in-sample only; the OOS run is a single run.

## C. Sanity against the author
Run the author's instrument and dates. Compare: direction of the curve, biggest DD location, number of
trades per year, win rate. Expect our numbers to be *worse* (futures costs, fixed-$ sizing, no
compounding). Expect similar *shape*. If the shape differs, suspect a rule you mis-read
(stop? long only? parameter?). Write both sets of numbers in the conclusion.

## D. Red flags (name them in the conclusion when present)
- Trades < ~200 in total, or < ~10 per year in the OOS window.
- One market > 40% of net profit, or the strategy is profitable in < 50% of markets.
- Most of the profit before 1990 (inflation trends, low liquidity, synthetic back-history).
- Profit factor < 1.2 after costs; average trade < 2x round-turn cost per contract.
- Sharpe IS > 1 that drops below 0.3 OOS; sign flip of MAR OOS.
- Max DD duration measured in years; DD > 30% of capital at $7,500 risk per trade.
- Sensitivity grid with a single good cell (cliff) instead of a region (plateau).
- Scale-in mean reversion with no stop: fat left tail (check Largest Loss vs risk per trade, trade skew).
- Intraday rules on daily bars; "exit same day" ideas; anything needing the session open.
- Author reports compounding / no costs / equal weight / stocks; our test uses none of those - say so.

## E. Writing the conclusion (Spanish, for Jacob)
1. Qué se probó y con qué supuestos (una lista corta, incluyendo lo que el autor no dijo).
2. Tabla: Full / IS / OOS con Avg Return % p.a., MaxDD %, MAR, Sharpe, trades, PF, % winners.
3. De dónde sale el P&L: mercados, grupos, años, long vs short.
4. Sensibilidad: meseta o pico.
5. Veredicto: sobrevive costos + OOS + sensibilidad, sí o no; qué cambiaría el veredicto; qué se
   necesita para operarlo (tipo de orden, datos, overlay de riesgo). Compara con las cifras del autor.
Keep it direct. A negative result is a valid, useful result.

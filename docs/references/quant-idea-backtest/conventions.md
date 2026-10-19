# Conventions (copied from Jacob's MATLAB framework: portfolio1.m, strategy1.m, ongoingSizing.m, tradeAssessment.m, dataNorm.m, avgTrueRange.m)

## Data: CSI Unfair Advantage, `<code>0@EAB.txt`
- Columns, no header: `yyyymmdd, open, high, low, close, volume, open_interest, expiry_yyyymmdd, contract_code`.
- Continuous contract built in UA: roll 3 trading days before expiry, "OI or Volume" criterion, roll next
  day when known, **Back Adjusted** (Panama/additive, "close new - close old" on roll day), all contract months,
  daily. Other variants in the folder (`03X1N`, `03X2N`) are 1st/2nd nearest contracts for term-structure
  work - ignore them.
- Because the adjustment is additive: P&L = (points) x BPV is exact; price *levels* are meaningless far
  back and can be negative (CL 2020-04, several markets in the 1980s). Use point/ATR-based indicators,
  never % returns, price ratios or log prices on the adjusted series.
- File name = `file_code` in `assets/universe.csv` + `0@EAB.txt` (e.g. `GC2` -> `GC20@EAB.txt`,
  `ES_` -> `ES_0@EAB.txt`). The loader tolerates `_` instead of `@`.
- Calendar: every market is reindexed to a common Mon-Fri calendar and forward-filled; `traded=False`
  on holidays/missing days -> no signals, no fills, zero P&L that day (MATLAB dataNorm.m).
- A roll is detected when `contract_code` changes; a position carried across pays one commission per contract.
- Quote-unit traps (correct for CSI data, different in IBKR): SI cents/oz (BPV 50), HG cents/lb (BPV 250),
  JY USD per 100 JPY (BPV 125,000). For live orders in IBKR these become 5,000 / 25,000 / 12,500,000.

## Pairs / ratios
Signals that need price levels or ratios (log spread, hedge ratio, cointegration tests) must use the
unadjusted first-nearest files `<code>03X1N.txt` (same CSI format). P&L still comes from the Panama series.
`pairs_backtest.py` does both and reports Engle-Granger and ADF p-values, half-life and the share of rolling
1-year windows that are stationary - a pair that is not cointegrated in-sample should not be traded as mean reversion.

## ATR
Simple average of True Range over 20 bars (`avgTrueRange.m`), TR = max(H, prevC) - min(L, prevC).
ATR at bar i includes bar i, so it is known at the close of i, not at its open. The engine uses ATR(i) for
close fills and ATR(i-1) for open fills.

## Sizing: constant dollar risk, no compounding
```
risk$   = risk_pct x initial_capital          0.0075 x 1,000,000 = 7,500   (Setup(2) x Setup(1))
ATR$    = BPV x max(ATR, min_move x 10)       (Min.ATR ticks = Setup(11))
N_full  = risk$ / (stop_ix x ATR$)            stop_ix = 2 (POSITION_SIZING.docx)
```
Fractional contracts allowed in simulation (`--integer_contracts` to floor). `N_full` is fixed at the
first fill of a trade; scale-ins are fractions of it. The equity never feeds back into sizing
(FixedFractional = 0). "Risk per trade" is a sizing reference, not a guaranteed loss limit unless
the strategy has a stop.

## Costs
- Commission `Setup(4)` = $7.5 per contract **round-turn** (charged half at each fill) **plus** $7.5 per
  contract per roll while in position (`Cost x (1 + rolls)` in strategy1.m).
- Slippage `Setup(5)` = 10% range-bound: buy at `min(High, px + (High - px) x 10%)`, sell at
  `max(Low, px - (px - Low) x 10%)`, plus `Setup(6)` ticks (0). Applied to open and stop fills.
- Fills at the close: MATLAB applies no slippage ("no time after Close"). `--slip_ticks_close` adds ticks
  if the user wants a harsher close-fill assumption (1 tick is a reasonable stress).
- No margin, financing or FX modelling: whole universe is USD-denominated.

## Metrics (non-compounding, `FixedFractional = 0`)
- daily return = dEquity / initial capital; Avg Return % p.a. = mean x 252; Vol = std x sqrt(252).
- Drawdown % = (equity - running peak) / initial capital (not / peak). MaxDD = min.
- Sharpe [0%] = AvgReturn / Vol; Sharpe semi-dev and Sortino as in portfolio1.m; MAR = AvgReturn / |MaxDD|.
- Lake ratio, Ulcer index, UPI, New-high %, Omega as in portfolio1.m.
- Monthly return = dEquity over the month / initial capital; yearly = sum; intra-year DD on cumulative sums.
- Time-window table = rolling sums of monthly returns over 1/3/6/12/24/60 months.
- Trade = flat -> flat, scale-ins are one trade; stats on net P&L after commissions.
- CAGR is shown for reference only; with fixed-$ sizing Avg Return % p.a. is the comparable number.

## In-sample / out-of-sample
Cut date = start + 0.8 x (end - start) of the portfolio calendar (printed, shaded in the chart, stored in
`summary.json`). Parameter grids run with `end=cut`. OOS metrics are reported once with base parameters.

## Equities (Norgate S&P 500 Current & Past)
- Export with `scripts/export_norgate_sp500.py`: OHLCV adjusted for capital events only, `Unadjusted Close`, `Dividend`
  (entitled if held at that day's close), `Index Constituent` (1 = member that day). Delisted symbols carry `-YYYYMM`.
- `Panel` builds wide arrays (dates x symbols), ATR20, a total-return index (`tr_index`) for signals that need % returns.
- Simulator: entries only while the symbol is a member (as of the signal bar); exits always; delisting closes at the
  last bar's close; dividends credited to longs / debited to shorts; commission $0.005 per share, min $1 per order;
  slippage 10% of the remaining range on open fills; cap 10% of initial capital notional per position.
- Sizing: shares = risk$ / (stop_ix x ATR20), fractional, no compounding; a strategy may pass explicit shares
  (pairs legs = risk$/2 each) or +-1 to let the engine size.
- Report conventions identical to futures; the equity chart overlays S&P 500 TR buy & hold on the same capital for shape.

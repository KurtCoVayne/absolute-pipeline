# The house template for a daily futures idea (agent-guidelines.md 3, 4, 15):
# per root, on each root's own sessions; signals in ATR units at the session's
# close; the fill at the next open (the preset's daily variant); house sizing
# N_full = risk / (stop_ix x ATR$), fixed capital, fractional contracts.
strategy tmpl_donchian_moo {
  data {
    class    future
    need     bars @1m from 2010-06-07 complete
    need     sessions, point_value
    require  reviewed
    prefer   latest
  }
  uses      calendar, bars, book
  calendars { session = sessions }
  clock { decide on session }

  universe {
    universe(A, T) :- session(A, T, _).
  }

  params {
    n_entry  : Count = 55 in 10..120
    n_exit   : Count = 20 in 5..60
    atr_rows : Count = 20
    risk     : Notional<USD> = 7500 USD
    stop_ix  : Scalar = 2
  }

  rules {
    rel atr(A: Future, @T, V: Price<USD>)
    atr(A, T, V) :- universe(A, T), V = mean(R) over (T1 in rows[atr_rows], true_range@session(A, T1, R)).
    rel hi(A: Future, @T, H: Price<USD>)
    hi(A, T, H) :- universe(A, T), H = max(P) over (T1 in prior_rows[n_entry], high@session(A, T1, P)).
    rel lo(A: Future, @T, L: Price<USD>)
    lo(A, T, L) :- universe(A, T), L = min(P) over (T1 in prior_rows[n_entry], low@session(A, T1, P)).
    rel xhi(A: Future, @T, H: Price<USD>)
    xhi(A, T, H) :- universe(A, T), H = max(P) over (T1 in prior_rows[n_exit], high@session(A, T1, P)).
    rel xlo(A: Future, @T, L: Price<USD>)
    xlo(A, T, L) :- universe(A, T), L = min(P) over (T1 in prior_rows[n_exit], low@session(A, T1, P)).
    rel size(A: Future, @T, N: Quantity<Contracts>)
    size(A, T, N) :- atr(A, T, V), V > 0 USD/contract, point_value(A, T, PV), PV > 0, N = risk / (stop_ix * V * PV).
  }

  decisions {
    silence hold
    orders  moo
    decide(T, hold(A, N)) :- close@session(A, T, P), hi(A, T, H), P > H, flat(A, T), size(A, T, N).
    decide(T, hold(A, M)) :- close@session(A, T, P), lo(A, T, L), P < L, flat(A, T), size(A, T, N), M = -N.
    decide(T, hold(A, 0 contracts)) :- held(A, T, Q), Q > 0 contracts, close@session(A, T, P), xlo(A, T, L), P < L.
    decide(T, hold(A, 0 contracts)) :- held(A, T, Q), Q < 0 contracts, close@session(A, T, P), xhi(A, T, H), P > H.
  }

  execution futures_house with { orders moo }
  report { calendar session   periods 252 per year }
}

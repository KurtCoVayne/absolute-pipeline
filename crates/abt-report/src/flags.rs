//! The red flags of the guideline (§12), each computed from the report
//! tables of a submission's plan: the row names the flag, its threshold,
//! the value found, whether it fires, and the table it came from. A flag
//! whose table is absent is `fires: None` ("not computable"), never
//! silently green.

use crate::site::Strategy;
use crate::stats::{num, pct};

#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    pub name: &'static str,
    pub threshold: &'static str,
    pub value: String,
    pub fires: Option<bool>,
    pub source: &'static str,
}

fn flag(name: &'static str, threshold: &'static str, source: &'static str, found: Option<(String, bool)>) -> Flag {
    match found {
        Some((value, fires)) => Flag {
            name,
            threshold,
            value,
            fires: Some(fires),
            source,
        },
        None => Flag {
            name,
            threshold,
            value: "not computable: the table is missing".into(),
            fires: None,
            source,
        },
    }
}

/// The guideline's rows, in its order.
pub fn red_flags(s: &Strategy) -> Vec<Flag> {
    let full = s.summary_row("base", "full");
    let ins = s.summary_row("base", "insample");
    let oos = s.summary_row("base", "oos");
    let mut out = Vec::new();

    // Too few trades: < 200 in total, or < 10 a year out of sample.
    let trades = full.and_then(|r| s.summary().and_then(|t| t.f64(r, "trades")));
    let oos_per_year = oos.and_then(|r| {
        let t = s.summary()?;
        Some(t.f64(r, "trades")? / t.f64(r, "years")?.max(1e-9))
    });
    out.push(flag(
        "Too few trades",
        "< 200 in total, or < 10 a year out of sample",
        "summary (trade count)",
        trades.map(|n| {
            let per = oos_per_year.map(|p| format!(", {} a year out of sample", num(p, 1))).unwrap_or_default();
            (format!("{} trades{}", num(n, 0), per), n < 200.0 || oos_per_year.is_some_and(|p| p < 10.0))
        }),
    ));

    // One market carries it.
    let markets: Option<Vec<(f64, f64)>> = s
        .table("contribution")
        .map(|t| t.rows_where("kind", "market").into_iter().filter_map(|i| Some((t.f64(i, "share")?, t.f64(i, "net_profit")?))).collect());
    out.push(flag(
        "One market carries it",
        "one market > 40 % of net profit, or profitable in < 50 % of markets",
        "contribution by entity",
        markets.filter(|m| !m.is_empty()).map(|m| {
            let max = m.iter().map(|x| x.0).fold(f64::MIN, f64::max);
            let profitable = m.iter().filter(|x| x.1 > 0.0).count() as f64 / m.len() as f64;
            (
                format!("largest share {}, profitable in {} of {} markets", pct(max), num(profitable * m.len() as f64, 0), m.len()),
                max > 0.4 || profitable < 0.5,
            )
        }),
    ));

    // Profits before 1990.
    let before_1990: Option<(f64, f64)> = s.table("subperiods").map(|t| {
        let mut before = 0.0;
        let mut total = 0.0;
        for i in 0..t.len() {
            let y: i32 = t.text(i, "period").get(..4).and_then(|y| y.parse().ok()).unwrap_or(9999);
            let np = t.f64(i, "net_profit").unwrap_or(0.0);
            total += np;
            if y < 1990 {
                before += np;
            }
        }
        (before, total)
    });
    out.push(flag(
        "Profits before 1990",
        "most of the net profit before 1990",
        "sub-period table",
        before_1990.map(|(b, t)| {
            let share = if t.abs() > 0.0 { b / t } else { 0.0 };
            (format!("{} of net profit before 1990", pct(share)), t > 0.0 && share > 0.5)
        }),
    ));

    // Thin edge.
    let pf = full.and_then(|r| s.summary()?.f64(r, "profit_factor"));
    let avg_vs_cost = s.table("trades").and_then(|t| {
        let avg = t.f64(t.find("stat", "avg_trade_usd")?, "value")?;
        let cost = t.f64(t.find("stat", "round_turn_cost_usd")?, "value")?;
        Some((avg, cost))
    });
    out.push(flag(
        "Thin edge",
        "profit factor < 1.2 after costs; average trade < 2 × round-turn cost",
        "trade statistics",
        pf.map(|pf| {
            let (text, thin) = match avg_vs_cost {
                Some((avg, cost)) => (
                    format!(", average trade {} against a round turn of {}", crate::stats::usd(avg), crate::stats::usd(cost)),
                    avg < 2.0 * cost,
                ),
                None => (String::new(), false),
            };
            (format!("profit factor {}{}", num(pf, 2), text), pf < 1.2 || thin)
        }),
    ));

    // Collapses out of sample.
    let pair = match (ins, oos) {
        (Some(i), Some(o)) => s.summary().and_then(|t| Some((t.f64(i, "sharpe")?, t.f64(o, "sharpe")?, t.f64(i, "mar")?, t.f64(o, "mar")?))),
        _ => None,
    };
    out.push(flag(
        "Collapses out of sample",
        "Sharpe in-sample > 1 falling below 0.3 out of sample; the sign of MAR flips",
        "the (IS, OOS) pair",
        pair.map(|(si, so, mi, mo)| {
            (
                format!("Sharpe {} → {}, MAR {} → {}", num(si, 2), num(so, 2), num(mi, 2), num(mo, 2)),
                (si > 1.0 && so < 0.3) || (mi.signum() != mo.signum() && mi != 0.0 && mo != 0.0),
            )
        }),
    ));

    // Deep or long drawdown.
    let dd = full.and_then(|r| {
        let t = s.summary()?;
        Some((t.f64(r, "max_drawdown")?, t.f64(r, "drawdown_duration").unwrap_or(0.0)))
    });
    out.push(flag(
        "Deep or long drawdown",
        "max drawdown > 30 % of capital; duration in years",
        "drawdown and its duration",
        dd.map(|(d, dur)| (format!("max drawdown {}, longest {} years", pct(d), num(dur, 1)), d > 0.30)),
    ));

    // Parameter cliff: the share of the best cell's neighbours within 10 %.
    out.push(flag(
        "Parameter cliff",
        "a single good cell instead of a region: < 50 % of the best cell's neighbours within 10 % of it",
        "parameter surface",
        s.table("surface")
            .and_then(neighbour_share)
            .map(|(share, n)| (format!("{} of {} neighbours within 10 % of the best cell", pct(share), n), share < 0.5)),
    ));

    // Fat left tail.
    let tail = s.table("trades").and_then(|t| {
        let largest = t.f64(t.find("stat", "largest_loss_usd")?, "value")?;
        let risk = t.f64(t.find("stat", "risk_per_trade_usd")?, "value")?;
        let skew = t.f64(t.find("stat", "skew")?, "value").unwrap_or(0.0);
        Some((largest.abs() / risk.max(1e-9), skew))
    });
    out.push(flag(
        "Fat left tail",
        "largest loss > 3 × risk per trade, or trade skew < −1",
        "trade statistics",
        tail.map(|(ratio, skew)| (format!("largest loss {} × risk per trade, skew {}", num(ratio, 1), num(skew, 2)), ratio > 3.0 || skew < -1.0)),
    ));

    // Intraday logic: from the rationale, not computable here.
    out.push(Flag {
        name: "Intraday logic forced onto daily bars",
        threshold: "\"exit same day\"; anything needing the session open",
        value: if s.submission.rationale.to_lowercase().contains("intraday") {
            "the rationale mentions intraday logic".into()
        } else {
            "not mentioned in the rationale".into()
        },
        fires: None,
        source: "the rationale (section 2 of the guideline)",
    });

    // Author's setup differs.
    let author = s.author_reported();
    out.push(flag(
        "Author's setup differs",
        "compounding, no costs, equal weight, stocks",
        "the comparison table",
        author.as_ref().map(|a| {
            let mut differs = Vec::new();
            if a.get("compounding").and_then(|v| v.as_bool()) == Some(true) {
                differs.push("compounding");
            }
            if a.get("costs").and_then(|v| v.as_bool()) == Some(false) {
                differs.push("no costs");
            }
            if a.get("sizing").and_then(|v| v.as_str()) == Some("equal") {
                differs.push("equal weight");
            }
            if a.get("instrument_class").and_then(|v| v.as_str()) == Some("stocks") {
                differs.push("stocks");
            }
            if differs.is_empty() {
                ("the author's setup matches the house rules".into(), false)
            } else {
                (differs.join(", "), true)
            }
        }),
    ));
    out
}

/// The share of the best cell's grid neighbours whose objective is within
/// 10 % of the best, over a surface whose first two columns are the
/// parameters and whose objective is `sharpe` (else its last column).
pub fn neighbour_share(t: &crate::Table) -> Option<(f64, usize)> {
    if t.columns.len() < 3 || t.is_empty() {
        return None;
    }
    let obj = t.col("sharpe").unwrap_or(t.columns.len() - 1);
    let (c1, c2) = (0usize, 1usize);
    let cells: Vec<(f64, f64, f64)> = (0..t.len())
        .filter_map(|i| Some((t.f64(i, &t.columns[c1])?, t.f64(i, &t.columns[c2])?, t.f64(i, &t.columns[obj])?)))
        .collect();
    let mut xs: Vec<f64> = cells.iter().map(|c| c.0).collect();
    let mut ys: Vec<f64> = cells.iter().map(|c| c.1).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xs.dedup();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ys.dedup();
    let best = cells.iter().max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))?;
    let ix = xs.iter().position(|x| *x == best.0)?;
    let iy = ys.iter().position(|y| *y == best.1)?;
    let mut n = 0;
    let mut within = 0;
    for dx in -1i64..=1 {
        for dy in -1i64..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (nx, ny) = (ix as i64 + dx, iy as i64 + dy);
            if nx < 0 || ny < 0 || nx >= xs.len() as i64 || ny >= ys.len() as i64 {
                continue;
            }
            let (x, y) = (xs[nx as usize], ys[ny as usize]);
            if let Some(c) = cells.iter().find(|c| c.0 == x && c.1 == y) {
                n += 1;
                if best.2 > 0.0 && c.2 >= best.2 * 0.9 {
                    within += 1;
                }
            }
        }
    }
    if n == 0 {
        return None;
    }
    Some((within as f64 / n as f64, n))
}

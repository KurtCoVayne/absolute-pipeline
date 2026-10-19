//! The arithmetic the site does itself: the cohort's Benjamini–Hochberg
//! adjustment (PL-6.6 item 7, Q-4.3's rule one level up), the calendar
//! week a submission falls in, and the number formats of the pages.
//! Everything else (Sharpe, DSR, the minimum track-record length) comes
//! from the query layer's tables and is shown, not recomputed.

use chrono::{DateTime, Datelike};
use pipeline_wire::Time;

/// Benjamini–Hochberg (1995): `q_(i) = min_{j ≥ i} (m / j) · p_(j)`, capped
/// at 1, returned in the input's order. An empty input is an empty output;
/// a p-value outside [0, 1] is clamped first.
pub fn benjamini_hochberg(p: &[f64]) -> Vec<f64> {
    let m = p.len();
    if m == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&a, &b| p[a].clamp(0.0, 1.0).partial_cmp(&p[b].clamp(0.0, 1.0)).unwrap_or(std::cmp::Ordering::Equal));
    let mut q = vec![0.0; m];
    let mut running = 1.0f64;
    for rank in (0..m).rev() {
        let i = order[rank];
        let raw = p[i].clamp(0.0, 1.0) * m as f64 / (rank + 1) as f64;
        running = running.min(raw).min(1.0);
        q[i] = running;
    }
    q
}

/// The ISO calendar week of a time, as `2026-W38`.
pub fn iso_week(t: Time) -> String {
    let secs = t.div_euclid(pipeline_wire::NS_PER_S);
    let nanos = t.rem_euclid(pipeline_wire::NS_PER_S) as u32;
    match DateTime::from_timestamp(secs, nanos) {
        Some(d) => {
            let w = d.iso_week();
            format!("{:04}-W{:02}", w.year(), w.week())
        }
        None => "unknown".into(),
    }
}

/// The calendar year of a time.
pub fn year(t: Time) -> i32 {
    DateTime::from_timestamp(t.div_euclid(pipeline_wire::NS_PER_S), 0).map(|d| d.year()).unwrap_or(0)
}

/// The date part of a time, `2026-09-18`.
pub fn date(t: Time) -> String {
    let s = pipeline_wire::iso(t);
    s[..10].to_string()
}

/// A number for a cell: grouped thousands, `digits` decimals (none when
/// `digits` is 0).
pub fn num(v: f64, digits: usize) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    if digits == 0 {
        return group(v.round() as i64);
    }
    let s = format!("{:.*}", digits, v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let int: i64 = int.parse().unwrap_or(0);
    let sign = if v < 0.0 && (int != 0 || frac.bytes().any(|b| b != b'0')) { "-" } else { "" };
    format!("{}{}.{}", sign, group(int), frac)
}

/// A parameter value as the grid names it: up to four decimals, trailing
/// zeros dropped.
pub fn param(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    if v.fract() == 0.0 {
        return group(v as i64);
    }
    let s = format!("{:.4}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn group(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{}", out)
    } else {
        out
    }
}

/// A share as a percentage with one decimal.
pub fn pct(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    format!("{:.1} %", v * 100.0)
}

pub fn usd(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    if v.abs() < 10.0 {
        format!("${:.3}", v)
    } else {
        format!("${}", num(v, 2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bh_of_ten_equals_the_hand_computation() {
        // Sorted: 0.001 0.008 0.012 0.03 0.04 0.045 0.2 0.3 0.6 0.9 with m = 10.
        // Raw m/j·p: 0.01 0.04 0.04 0.075 0.08 0.075 0.2857 0.375 0.6667 0.9;
        // the running minimum from the top: 0.9 0.6667 0.375 0.2857 0.075 0.075 0.075 0.04 0.04 0.01.
        let p = [0.03, 0.001, 0.9, 0.012, 0.2, 0.045, 0.6, 0.008, 0.04, 0.3];
        let q = benjamini_hochberg(&p);
        let want = [0.075, 0.01, 0.9, 0.04, 0.2857142857142857, 0.075, 0.6666666666666666, 0.04, 0.075, 0.375];
        for (a, b) in q.iter().zip(want.iter()) {
            assert!((a - b).abs() < 1e-12, "{:?} != {:?}", q, want);
        }
        assert!(benjamini_hochberg(&[]).is_empty());
        assert_eq!(benjamini_hochberg(&[0.5]), vec![0.5]);
    }

    #[test]
    fn weeks_and_numbers() {
        assert_eq!(iso_week(pipeline_wire::parse_iso("2026-09-18T19:12:36Z").unwrap()), "2026-W38");
        assert_eq!(iso_week(pipeline_wire::parse_iso("2026-01-01").unwrap()), "2026-W01");
        assert_eq!(num(1234567.0, 0), "1,234,567");
        assert_eq!(num(1234567.0, 2), "1,234,567.00");
        assert_eq!(num(-12.345, 2), "-12.35");
        assert_eq!(num(-0.001, 2), "0.00");
        assert_eq!(param(0.005), "0.005");
        assert_eq!(param(250.0), "250");
        assert_eq!(pct(0.4123), "41.2 %");
        assert_eq!(usd(1234.5), "$1,234.50");
        assert_eq!(year(pipeline_wire::parse_iso("1989-12-31").unwrap()), 1989);
    }
}

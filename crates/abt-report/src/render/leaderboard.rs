//! The Leaderboard (PL-P1, PL-7.4): submissions ordered by forward
//! evidence, the forward DSR then the track-record length, never by
//! in-sample Sharpe; in-sample-only submissions in a separate list below;
//! the cohort's Benjamini–Hochberg adjustment of the forward claims per
//! calendar week of submission as a column (PL-6.6 item 7).

use maud::{html, Markup};

use super::{account_path, frame, opt_num, status, strategy_path};
use crate::site::{LeaderRow, Site};
use crate::stats::num;

pub fn page(site: &Site) -> Markup {
    let body = html! {
        h1 { "Leaderboard" }
        p.lede { "Ordered by forward evidence: the deflated Sharpe over the forward window (H, now], counted against the lineage's trials, then the length of the track record. In-sample Sharpe is shown for reference and orders nothing. The q column is the Benjamini–Hochberg adjustment of the forward claim's p-value across every submission of the same calendar week, so a week of fifty hypotheses cannot manufacture a winner by volume." }
        h2 { "With forward evidence" }
        @if site.leaderboard.is_empty() { p.muted { "No reveal yet." } } @else { (forward_table(site, "", &site.leaderboard)) }
        h2 { "In-sample only" }
        p.muted { "Submitted, pre-registered, and waiting for the scheduler's first reveal; newest first. Nothing here is evidence." }
        @if site.insample_only.is_empty() { p.muted { "None." } } @else { (insample_table(&site.insample_only)) }
    };
    frame(site, "", "Leaderboard", body)
}

pub fn forward_table(site: &Site, root: &str, rows: &[LeaderRow]) -> Markup {
    let _ = site;
    html! {
        table.data.leaderboard {
            thead { tr {
                th { "#" } th { "Strategy" } th { "Account" } th { "Hypothesis" } th.num { "Forward DSR" } th.num { "Track record" } th.num { "Forward Sharpe" } th.num { "Reveals" }
                th.num { "p (forward claim)" } th.num { "q (BH, week cohort)" } th { "Week" } th.num { "In-sample Sharpe" } th { "Status" }
            } }
            tbody {
                @for (i, r) in rows.iter().enumerate() {
                    tr.leader data-dsr=(opt_num(r.dsr, 4)) data-track=(opt_num(r.track_observed, 0)) {
                        td { (i + 1) }
                        td { a href=(format!("{}{}", root, strategy_path(&r.submission_id))) title=(r.submission_id) { code { (super::short(&r.submission_id)) } } }
                        td { a href=(format!("{}{}", root, account_path(&r.handle))) { "@" (r.handle) } }
                        td { (truncate(&r.statement, 90)) }
                        td.num { (opt_num(r.dsr, 3)) }
                        td.num { (opt_num(r.track_observed, 0)) @if let Some(req) = r.track_required { " / " (num(req, 0)) } }
                        td.num { (opt_num(r.forward_sharpe, 2)) }
                        td.num { (r.reveals) }
                        td.num { (opt_num(r.p_value, 4)) }
                        td.num { (opt_num(r.q_value, 4)) @if r.cohort > 0 { " " small.muted { "(" (r.cohort) ")" } } }
                        td { (r.week) }
                        td.num.muted { (opt_num(r.insample_sharpe, 2)) }
                        td { (status(&r.status)) }
                    }
                }
            }
        }
    }
}

fn insample_table(rows: &[LeaderRow]) -> Markup {
    html! {
        table.data {
            thead { tr { th { "Strategy" } th { "Account" } th { "Hypothesis" } th { "Week" } th.num { "In-sample Sharpe" } th { "Status" } } }
            tbody {
                @for r in rows {
                    tr {
                        td { a href=(strategy_path(&r.submission_id)) title=(r.submission_id) { code { (super::short(&r.submission_id)) } } }
                        td { a href=(account_path(&r.handle)) { "@" (r.handle) } }
                        td { (truncate(&r.statement, 90)) }
                        td { (r.week) }
                        td.num.muted { (opt_num(r.insample_sharpe, 2)) }
                        td { (status(&r.status)) }
                    }
                }
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

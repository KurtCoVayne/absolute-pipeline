//! The accounts: per account its hypotheses, submissions, the forward hit
//! rate of its claims, the volume by week and the cost it caused.

use maud::{html, Markup};

use super::{account_path, document_path, frame, hypothesis_path, strategy_path};
use crate::site::{Account, Site};
use crate::stats::{num, pct, usd};

fn hit_rate(a: &Account) -> Option<f64> {
    let n = a.forward_holds + a.forward_fails;
    (n > 0).then(|| a.forward_holds as f64 / n as f64)
}

pub fn index(site: &Site) -> Markup {
    let mut accounts: Vec<&Account> = site.accounts.values().collect();
    accounts.sort_by(|a, b| {
        b.submissions
            .len()
            .cmp(&a.submissions.len())
            .then_with(|| b.documents.len().cmp(&a.documents.len()))
            .then_with(|| a.handle.cmp(&b.handle))
    });
    let body = html! {
        h1 { "Accounts" }
        p.lede { "Every followed account and every author on the log: how much it posts, how many hypotheses and submissions it caused, how its forward claims fare, and what it cost." }
        table.data {
            thead { tr { th { "Account" } th { "Tier" } th.num { "Documents" } th.num { "Candidates" } th.num { "Hypotheses" } th.num { "Submissions" } th.num { "Forward hit rate" } th.num { "Cost" } } }
            tbody {
                @for a in accounts {
                    tr {
                        td { a href=(account_path(&a.handle)) { "@" (a.handle) } }
                        td { (a.tier.map(|t| t.to_string()).unwrap_or_default()) }
                        td.num { (a.documents.len()) }
                        td.num { (a.verdicts.get("candidate").copied().unwrap_or(0)) }
                        td.num { (a.hypotheses.len()) }
                        td.num { (a.submissions.len()) }
                        td.num { (hit_rate(a).map(pct).unwrap_or_else(|| "—".into())) @if a.forward_holds + a.forward_fails > 0 { " " small.muted { "(" (a.forward_holds) "/" (a.forward_holds + a.forward_fails) ")" } } }
                        td.num { (usd(a.cost_usd)) }
                    }
                }
            }
        }
    };
    frame(site, "", "Accounts", body)
}

pub fn page(site: &Site, a: &Account) -> Markup {
    let root = "../";
    let follow = site.inputs.follows.iter().find(|f| f.handle == a.handle);
    let body = html! {
        h1 { "@" (a.handle) }
        dl.facts {
            dt { "Account id" } dd { (a.account_id) }
            @if let Some(f) = follow {
                dt { "Follow" } dd { "tier " (f.tier) " · " (format!("{:?}", f.state).to_lowercase()) " · added by " (f.added_by) " · shared " (f.shared) " times (" (f.posts_shared) " posts)" }
            }
            dt { "Documents" } dd { (a.documents.len()) }
            dt { "Triage" } dd { @for (k, v) in &a.verdicts { (k) " " (v) "; " } }
            dt { "Hypotheses" } dd { (a.hypotheses.len()) }
            dt { "Submissions" } dd { (a.submissions.len()) }
            dt { "Forward hit rate" } dd { (hit_rate(a).map(pct).unwrap_or_else(|| "no forward claim yet".into())) " " small.muted { "(" (a.forward_holds) " holds, " (a.forward_fails) " fails over the latest reveals)" } }
            dt { "Cost" } dd { (usd(a.cost_usd)) " " small.muted { "(triage, analysis and strategy jobs on this account's documents)" } }
        }
        h2 { "Volume by week" }
        table.data {
            thead { tr { th { "Week" } th.num { "Documents" } } }
            tbody { @for (w, n) in &a.volume { tr { td { (w) } td.num { (n) } } } }
        }
        h2 { "Submissions" }
        @if a.submissions.is_empty() { p.muted { "None." } } @else {
            table.data {
                thead { tr { th { "Strategy" } th { "Hypothesis" } th.num { "Forward DSR" } th.num { "Reveals" } th { "Status" } } }
                tbody {
                    @for id in &a.submissions {
                        @if let Some(s) = site.strategy(id) {
                            tr {
                                td { a href=(format!("{}{}", root, strategy_path(id))) title=(id) { code { (super::short(id)) } } }
                                td { (s.hypothesis.map(|h| h.statement.clone()).unwrap_or_default()) }
                                td.num { (super::opt_num(s.forward_dsr(), 3)) }
                                td.num { (s.outcomes.len()) }
                                td { (super::status(&s.status())) }
                            }
                        }
                    }
                }
            }
        }
        h2 { "Hypotheses" }
        @if a.hypotheses.is_empty() { p.muted { "None." } } @else {
            ul { @for id in &a.hypotheses { li { a href=(format!("{}{}", root, hypothesis_path(id))) title=(id) { code { (super::short(id)) } } ": " (site.inputs.hypotheses.get(id).map(|h| h.statement.clone()).unwrap_or_default()) } } }
        }
        h2 { "Documents" }
        table.data {
            thead { tr { th { "Fixed at" } th { "Document" } th { "Verdict" } th.num { "Score" } } }
            tbody {
                @for id in &a.documents {
                    @if let Some(d) = site.inputs.documents.get(id) {
                        tr {
                            td { (super::time(d.fixed_at)) }
                            td { a href=(format!("{}{}", root, document_path(id))) title=(id) { code { (super::short(id)) } } " " @if d.source_deleted { span.deleted { "source deleted" } } @else { (d.text.chars().take(100).collect::<String>()) } }
                            td { (site.triage_of_doc.get(id).map(|t| super::status(Site::verdict_name(t.verdict))).unwrap_or_else(|| super::status("untriaged"))) }
                            td.num { (site.triage_of_doc.get(id).map(|t| num(t.score, 2)).unwrap_or_default()) }
                        }
                    }
                }
            }
        }
    };
    frame(site, root, &format!("@{}", a.handle), body)
}

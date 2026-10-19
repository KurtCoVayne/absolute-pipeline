//! Operations: budgets consumed per stage and campaign against the
//! `BudgetRow`s, the queue lags the audit shows, the dead-letter queues'
//! contents, the compliance actions (tombstones), the machine's facts.

use std::collections::BTreeMap;

use maud::{html, Markup};
use serde_json::Value;

use super::{frame, id_link, status, table_markup, time};
use crate::site::Site;
use crate::stats::{num, usd};

pub fn page(site: &Site) -> Markup {
    let i = site.inputs;
    // Consumed per stage from the records' costs.
    let triage: f64 = i.triage.iter().map(|t| t.cost_usd).sum();
    let analyze: f64 = i.hypotheses.values().map(|h| h.cost_usd).sum();
    let strategize: f64 = i.submissions.values().map(|s| s.cost_usd).sum();
    let mut campaigns: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    for s in i.submissions.values() {
        let c = campaigns.entry(s.campaign.clone().unwrap_or_else(|| "(none)".into())).or_default();
        c.0 += s.cost_usd;
        c.1 += 1;
    }
    let consumed = |scope: &str| -> Option<f64> {
        match scope {
            "global" | "stage:*" => Some(triage + analyze + strategize),
            "stage:triage" => Some(triage),
            "stage:analyze" => Some(analyze),
            "stage:strategize" => Some(strategize),
            s => s.strip_prefix("campaign:").and_then(|c| campaigns.get(c).map(|x| x.0)),
        }
    };
    let audit = i.pipeline.get("pipeline.audit");
    let body = html! {
        h1 { "Operations" }
        h2 { "Budgets" }
        p.muted { "Consumed from the records' costs (every triage, hypothesis and submission names its cost); caps from the control topic's budget rows." }
        table.data {
            thead { tr { th { "Scope" } th.num { "Consumed" } th.num { "Cap" } th { "Period" } th { "Set by" } th { "Since" } th.num { "Used" } } }
            tbody {
                tr { td { "stage:triage" } td.num { (usd(triage)) } (cap_cells(i, "stage:triage", triage)) }
                tr { td { "stage:analyze" } td.num { (usd(analyze)) } (cap_cells(i, "stage:analyze", analyze)) }
                tr { td { "stage:strategize" } td.num { (usd(strategize)) } (cap_cells(i, "stage:strategize", strategize)) }
                @for (c, (cost, n)) in &campaigns {
                    tr { td { "campaign:" (c) " " small.muted { "(" (n) " submissions)" } } td.num { (usd(*cost)) } (cap_cells(i, &format!("campaign:{}", c), *cost)) }
                }
                tr { td { "global" } td.num { (usd(triage + analyze + strategize)) } (cap_cells(i, "global", triage + analyze + strategize)) }
                @for b in i.budgets.iter().filter(|b| consumed(&b.scope).is_none()) {
                    tr { td { (b.scope) } td.num { "—" } td.num { (usd(b.cap_usd)) } td { (b.period) } td { (b.set_by) } td { (time(b.r)) } td.num { "—" } }
                }
            }
        }
        @if !i.campaigns.is_empty() {
            h3 { "Campaigns" }
            table.data {
                thead { tr { th { "Campaign" } th { "Skills version" } th.num { "Budget" } th.num { "Posts" } th { "Started" } th { "Finished" } } }
                tbody { @for c in &i.campaigns { tr { td { (c.name) " " code { (super::short(&c.id)) } } td { (c.skills_version) } td.num { (usd(c.budget_usd)) } td.num { (c.posts.as_ref().map(|p| p.len()).unwrap_or(0)) } td { (time(c.started)) } td { (time(c.finished)) } } } }
            }
        }
        h2 { "Queue lags" }
        @match audit {
            Some(t) => (lags(t)),
            None => p.muted { "The data layer's audit relation was not read (no data socket, or the `pipeline` class is not there yet)." }
        }
        h2 { "Dead-letter queues" }
        @if i.dlq.is_empty() { p.muted { "No dead-letter reader was given, or the queues are empty." } } @else {
            table.data {
                thead { tr { th { "Job" } th { "Stage" } th { "Agent" } th { "Inputs" } th.num { "Attempt" } th { "State" } th { "Failure" } th { "Finished" } th.num { "Cost" } } }
                tbody {
                    @for j in &i.dlq {
                        tr id=(format!("job-{}", j.id)) {
                            td { code title=(j.id) { (super::short(&j.id)) } }
                            td { (format!("{:?}", j.stage).to_lowercase()) }
                            td { (j.agent) " · " (j.model) }
                            td { @for x in j.inputs.iter().flatten() { (id_link(site, "", x)) " " } }
                            td.num { (j.attempt) }
                            td { (status(&format!("{:?}", j.state).to_lowercase())) }
                            td { @if let Some(f) = &j.failure { strong { (f.reason) } @if let Some(d) = &f.detail { ": " (d) } @if f.retryable { " (retryable)" } } }
                            td { (time(j.finished)) }
                            td.num { (j.cost.as_ref().map(|c| usd(c.money_usd)).unwrap_or_default()) }
                        }
                    }
                }
            }
        }
        h2 { "Compliance" }
        @if i.tombstones.is_empty() { p.muted { "No tombstone." } } @else {
            table.data {
                thead { tr { th { "Tombstone" } th { "Subject" } th { "Subject id" } th { "Document" } th { "Reason" } th { "At" } } }
                tbody {
                    @for t in &i.tombstones {
                        tr {
                            td { code title=(t.id) { (super::short(&t.id)) } }
                            td { (format!("{:?}", t.subject).to_lowercase()) }
                            td { @if matches!(t.subject, pipeline_wire::TombstoneSubject::Document) { (id_link(site, "", &t.subject_id)) } @else { (t.subject_id) } }
                            td { @if let Some(d) = &t.doc_id { (id_link(site, "", d)) } }
                            td { (t.reason) }
                            td { (time(t.r)) }
                        }
                    }
                }
            }
        }
        h2 { "Follow list" }
        table.data {
            thead { tr { th { "Handle" } th { "Account id" } th.num { "Tier" } th { "State" } th { "Added by" } th.num { "Shared" } th { "Since" } } }
            tbody { @for f in &i.follows { tr { td { a href=(super::account_path(&f.handle)) { "@" (f.handle) } } td { (f.account_id) } td.num { (f.tier) } td { (status(&format!("{:?}", f.state).to_lowercase())) } td { (f.added_by) } td.num { (f.shared) } td { (time(f.r)) } } } }
        }
        h2 { "The machine" }
        @match &i.facts {
            Some(f) => (facts(f)),
            None => p.muted { "No facts file was given (`--facts FILE`)." }
        }
    };
    frame(site, "", "Operations", body)
}

fn cap_cells(i: &crate::Inputs, scope: &str, consumed: f64) -> Markup {
    match i.budgets.iter().find(|b| b.scope == scope) {
        Some(b) => html! {
            td.num { (usd(b.cap_usd)) } td { (b.period) } td { (b.set_by) } td { (time(b.r)) }
            td.num { @if b.cap_usd > 0.0 { (crate::stats::pct(consumed / b.cap_usd)) @if consumed > b.cap_usd { " " (status("over")) } } @else { "—" } }
        },
        None => html! { td.num { "no cap" } td { } td { } td { } td.num { "—" } },
    }
}

/// Queue lags from the audit: per request kind, the count, the mean and
/// the maximum of a `queued_ms` (or `queue_ms`) column when the audit has
/// one, else the durations alone.
fn lags(t: &crate::Table) -> Markup {
    let kind_col = ["kind", "request", "request_kind"].iter().find(|c| t.col(c).is_some()).copied();
    let queue_col = ["queued_ms", "queue_ms", "wait_ms"].iter().find(|c| t.col(c).is_some()).copied();
    let dur_col = ["duration_ms", "duration"].iter().find(|c| t.col(c).is_some()).copied();
    let Some(kind_col) = kind_col else {
        return html! { p.muted { "The audit has " (t.len()) " rows and no request-kind column." } (table_markup(t)) };
    };
    let mut by_kind: BTreeMap<String, (usize, f64, f64, f64, f64)> = BTreeMap::new();
    for i in 0..t.len() {
        let e = by_kind.entry(t.text(i, kind_col)).or_insert((0, 0.0, 0.0, 0.0, 0.0));
        e.0 += 1;
        if let Some(q) = queue_col.and_then(|c| t.f64(i, c)) {
            e.1 += q;
            e.2 = e.2.max(q);
        }
        if let Some(d) = dur_col.and_then(|c| t.f64(i, c)) {
            e.3 += d;
            e.4 = e.4.max(d);
        }
    }
    html! {
        @if queue_col.is_none() { p.muted { "The audit shows durations and no queue column; lags are not shown." } }
        table.data {
            thead { tr { th { "Request" } th.num { "Count" } th.num { "Mean queued (ms)" } th.num { "Max queued (ms)" } th.num { "Mean duration (ms)" } th.num { "Max duration (ms)" } } }
            tbody {
                @for (k, (n, q, qm, d, dm)) in &by_kind {
                    tr { td { (k) } td.num { (n) }
                        td.num { @if queue_col.is_some() { (num(q / *n as f64, 1)) } @else { "—" } }
                        td.num { @if queue_col.is_some() { (num(*qm, 1)) } @else { "—" } }
                        td.num { @if dur_col.is_some() { (num(d / *n as f64, 1)) } @else { "—" } }
                        td.num { @if dur_col.is_some() { (num(*dm, 1)) } @else { "—" } }
                    }
                }
            }
        }
    }
}

fn facts(v: &Value) -> Markup {
    match v.as_object() {
        Some(m) => html! {
            dl.facts {
                @for (k, v) in m {
                    dt { (k) }
                    dd { @match v { Value::Object(_) | Value::Array(_) => pre { (serde_json::to_string_pretty(v).unwrap_or_default()) }, other => (super::cell(other)) } }
                }
            }
        },
        None => html! { pre { (v.to_string()) } },
    }
}

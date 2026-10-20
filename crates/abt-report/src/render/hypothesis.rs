//! A hypothesis's page: the statement, the evidence spans linked into the
//! documents' text, `H`, the data needs, the claims, and the submission it
//! led to or the failure that explains why none.

use maud::{html, Markup};
use pipeline_wire::Hypothesis;

use super::{account_path, document_path, frame, id_link, strategy_path, time};
use crate::site::Site;

pub fn page(site: &Site, h: &Hypothesis) -> Markup {
    let root = "../";
    let strategy = site.strategy_of_hypothesis.get(&h.id);
    let failure = site.inputs.dlq.iter().find(|j| j.inputs.iter().flatten().any(|i| i == &h.id));
    let handle = h.doc_ids.iter().flatten().find_map(|d| site.inputs.documents.get(d)).map(|d| d.author.handle.clone());
    let body = html! {
        h1 { "Hypothesis " code { (super::short(&h.id)) } }
        p.lede { (h.statement) }
        dl.facts {
            dt { "Id" } dd { code { (h.id) } }
            dt { "H" } dd { (time(h.h)) " " small.muted { "(the information horizon: in-sample before, forward after)" } }
            dt { "Account" } dd { @if let Some(a) = &handle { a href=(format!("{}{}", root, account_path(a))) { "@" (a) } } }
            dt { "Universe" } dd { (h.universe) }
            dt { "Signal" } dd { (h.signal) }
            dt { "Horizon" } dd { (h.horizon.clone().unwrap_or_default()) }
            dt { "Direction" } dd { (format!("{:?}", h.direction)) }
            dt { "Confidence" } dd { (crate::stats::num(h.confidence, 2)) }
            dt { "Data needs" } dd { @for n in h.data_needs.iter().flatten() { code { (n) } " " } }
            dt { "Agent" } dd { (h.agent) " · " (h.model) " · prompt " code { (super::short(&h.prompt_hash)) } " · skills " (h.skills.iter().flatten().cloned().collect::<Vec<_>>().join(", ")) }
            dt { "Cost" } dd { (crate::stats::usd(h.cost_usd)) }
            dt { "Received" } dd { (time(h.r)) @if let Some(j) = &h.job_id { " · job " (id_link(site, root, j)) } }
        }
        h2 { "Documents" }
        ul {
            @for d in h.doc_ids.iter().flatten() {
                li { (id_link(site, root, d))
                    @if let Some(doc) = site.inputs.documents.get(d) {
                        " @" (doc.author.handle) ", fixed " (time(doc.fixed_at))
                        @if doc.source_deleted { " " span.deleted { "source deleted" } }
                    } @else { " " span.muted { "(not on the log)" } }
                }
            }
        }
        h2 { "Evidence" }
        @if h.evidence.as_ref().is_none_or(|e| e.is_empty()) { p.muted { "No spans cited." } }
        @else {
            table.data {
                thead { tr { th { "Document" } th { "Span" } th { "Quote" } } }
                tbody {
                    @for e in h.evidence.iter().flatten() {
                        tr {
                            td { a href=(format!("{}{}", root, document_path(&e.doc_id))) title=(e.doc_id) { code { (super::short(&e.doc_id)) } } }
                            td { (e.span_from) "–" (e.span_to) }
                            td { @if site.inputs.documents.get(&e.doc_id).is_some_and(|d| d.source_deleted) { span.deleted { "source deleted" } } @else { (e.quote.clone().unwrap_or_default()) } }
                        }
                    }
                }
            }
        }
        @if let Some(a) = &h.author_reported {
            h2 { "The author's numbers" }
            (author_table(a))
        }
        h2 { "Claims to pre-register" }
        @if h.claims.as_ref().is_none_or(|c| c.is_empty()) { p.muted { "None." } }
        @else { pre { @for c in h.claims.iter().flatten() { (c) "\n" } } }
        h2 { "Outcome" }
        @match strategy {
            Some(s) => { p { "Submission " a href=(format!("{}{}", root, strategy_path(s))) title=(s) { code { (super::short(s)) } } " — the strategy page shows its in-sample results and the forward track." } }
            None => {
                @match failure {
                    Some(j) => {
                        p { "No submission: the strategize job " (id_link(site, root, &j.id)) " failed." }
                        @if let Some(f) = &j.failure { p { strong { (f.reason) } @if let Some(d) = &f.detail { ": " (d) } @if f.retryable { " (retryable)" } } }
                    }
                    None => { p.muted { "No submission yet: no strategize job has written one, and none failed on the dead-letter topic." } }
                }
            }
        }
    };
    frame(site, root, &format!("Hypothesis {}", super::short(&h.id)), body)
}

/// The author's reported numbers, as a definition list from the JSON text.
pub fn author_table(a: &str) -> Markup {
    let v: serde_json::Value = serde_json::from_str(a).unwrap_or(serde_json::Value::Null);
    match v.as_object() {
        Some(m) => html! { dl.facts { @for (k, v) in m { dt { (k) } dd { (super::cell(v)) } } } },
        None => html! { pre { (a) } },
    }
}

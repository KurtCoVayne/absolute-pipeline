//! The Feed: recent documents with their triage verdicts and flags,
//! filters by account, instrument and verdict (a few lines of script; the
//! page reads whole without it), each row linking to the document, its
//! hypotheses and the strategies it caused.

use maud::{html, Markup};
use pipeline_wire::Document;

use super::{account_path, document_path, frame, hypothesis_path, status, strategy_path, time};
use crate::site::Site;
use crate::stats::num;

const FILTER_SCRIPT: &str = r#"
(function () {
  var selects = document.querySelectorAll('.filters select');
  function apply() {
    var want = {};
    selects.forEach(function (s) { want[s.dataset.key] = s.value; });
    document.querySelectorAll('tr[data-account]').forEach(function (row) {
      var show = Object.keys(want).every(function (k) {
        return want[k] === '' || (' ' + (row.dataset[k] || '') + ' ').indexOf(' ' + want[k] + ' ') >= 0;
      });
      row.style.display = show ? '' : 'none';
    });
  }
  selects.forEach(function (s) { s.addEventListener('change', apply); });
})();
"#;

pub fn flags_of(d: &Document) -> Vec<String> {
    let mut out = Vec::new();
    if d.source_deleted {
        out.push("source deleted".into());
    }
    if let Some(f) = &d.flags {
        if f.injection >= 0.5 {
            out.push(format!("injection {:.2}", f.injection));
        }
        if f.spam >= 0.5 {
            out.push(format!("spam {:.2}", f.spam));
        }
        if f.dup_of.as_deref().is_some_and(|s| !s.is_empty()) {
            out.push("duplicate".into());
        }
        if f.near_dup_of.as_deref().is_some_and(|s| !s.is_empty()) {
            out.push("near duplicate".into());
        }
    }
    out
}

pub fn instruments_of(site: &Site, d: &Document) -> Vec<String> {
    let mut out: Vec<String> = d.entities.iter().flat_map(|e| e.instruments.iter().flatten()).map(|i| i.symbol.clone()).collect();
    if let Some(t) = site.triage_of_doc.get(&d.id) {
        for i in t.instruments.iter().flatten() {
            if !out.contains(i) {
                out.push(i.clone());
            }
        }
    }
    out
}

pub fn page(site: &Site) -> Markup {
    let docs = site.documents_by_recency();
    let accounts: Vec<&String> = site.accounts.keys().collect();
    let instruments = site.instruments();
    let body = html! {
        h1 { "Feed" }
        p.lede { (docs.len()) " documents from " (accounts.len()) " accounts, newest first, with the triage verdict and the flags of each. A row links to the document's text, its hypotheses and the strategies it led to." }
        form.filters {
            label { "Account " select data-key="account" { option value="" { "all" } @for a in &accounts { option value=(a) { "@" (a) } } } }
            label { "Instrument " select data-key="instrument" { option value="" { "all" } @for i in &instruments { option value=(i) { (i) } } } }
            label { "Verdict " select data-key="verdict" { option value="" { "all" } @for v in ["candidate", "claim", "commentary", "none", "untriaged"] { option value=(v) { (v) } } } }
            noscript { span.muted { "(filters need JavaScript; every row is shown)" } }
        }
        table.data id="feed" {
            thead { tr { th { "Fixed at" } th { "Account" } th { "Document" } th { "Instruments" } th { "Verdict" } th { "Score" } th { "Flags" } th { "Hypotheses" } th { "Strategies" } } }
            tbody {
                @for d in &docs {
                    (row(site, d))
                }
            }
        }
        script { (maud::PreEscaped(FILTER_SCRIPT)) }
    };
    frame(site, "", "Feed", body)
}

fn row(site: &Site, d: &Document) -> Markup {
    let triage = site.triage_of_doc.get(&d.id);
    let verdict = triage.map(|t| Site::verdict_name(t.verdict)).unwrap_or("untriaged");
    let instruments = instruments_of(site, d);
    let flags = flags_of(d);
    let hyps = site.hypotheses_of_doc.get(&d.id).cloned().unwrap_or_default();
    let strategies = site.strategies_of_doc.get(&d.id).cloned().unwrap_or_default();
    let excerpt: String = if d.source_deleted { String::new() } else { d.text.chars().take(140).collect() };
    html! {
        tr id=(format!("doc-{}", d.id)) data-account=(d.author.handle) data-instrument=(instruments.join(" ")) data-verdict=(verdict) {
            td { (time(d.fixed_at)) }
            td { a href=(account_path(&d.author.handle)) { "@" (d.author.handle) } }
            td {
                a href=(document_path(&d.id)) title=(d.id) { code { (super::short(&d.id)) } }
                " "
                @if d.source_deleted { span.deleted { "source deleted" } } @else { (excerpt) @if d.text.chars().count() > 140 { "…" } }
                @if d.version > 1 { " " small.muted { "v" (d.version) } }
                @if d.parents.as_ref().is_some_and(|p| !p.is_empty()) { " " small.muted { "(" (d.parents.as_ref().map(|p| p.len()).unwrap_or(0)) " parents)" } }
            }
            td { (instruments.join(", ")) }
            td { (status(verdict)) }
            td.num { (triage.map(|t| num(t.score, 2)).unwrap_or_else(|| "—".into())) }
            td { @for f in &flags { (status(f)) " " } }
            td { @for h in &hyps { a href=(hypothesis_path(h)) title=(h) { code { (super::short(h)) } } " " } }
            td { @for s in &strategies { a href=(strategy_path(s)) title=(s) { code { (super::short(s)) } } " " } }
        }
    }
}

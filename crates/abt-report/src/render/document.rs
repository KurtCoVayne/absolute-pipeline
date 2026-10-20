//! A document's page: the text with the evidence spans the hypotheses
//! cite marked, the segments, entities, links and parents, the triage
//! verdict, and the walk forward to its hypotheses and strategies. A
//! tombstoned document shows its id, its hashes and `source deleted`.

use maud::{html, Markup};
use pipeline_wire::Document;

use super::{account_path, document_path, feed, frame, hypothesis_path, id_link, status, strategy_path, time};
use crate::site::Site;

pub fn page(site: &Site, d: &Document) -> Markup {
    let root = "../";
    let hyps = site.hypotheses_of_doc.get(&d.id).cloned().unwrap_or_default();
    let strategies = site.strategies_of_doc.get(&d.id).cloned().unwrap_or_default();
    let triage = site.triage_of_doc.get(&d.id);
    // The spans the hypotheses cite, as (from, to, hypothesis id).
    let mut spans: Vec<(u32, u32, String)> = Vec::new();
    for h in &hyps {
        if let Some(hy) = site.inputs.hypotheses.get(h) {
            for e in hy.evidence.iter().flatten() {
                if e.doc_id == d.id && e.span_to > e.span_from {
                    spans.push((e.span_from, e.span_to, h.clone()));
                }
            }
        }
    }
    spans.sort();
    // The post's own URL when the adapter attached it (`attachments[kind = "permalink"]`, PL-D123).
    let post_url = d.attachments.iter().flatten().find(|a| a.kind == "permalink").map(|a| a.url.clone());
    let body = html! {
        h1 { "Document " code { (super::short(&d.id)) } }
        dl.facts {
            dt { "Id" } dd { code { (d.id) } }
            dt { "Version" } dd { (d.version) }
            dt { "Source" } dd { (format!("{:?}", d.source)) @if let Some(u) = &post_url { " · " a href=(u) rel="noreferrer" { (u) } } }
            dt { "Author" } dd { a href=(format!("{}{}", root, account_path(&d.author.handle))) { "@" (d.author.handle) } " " small.muted { "(" (d.author.id) ")" } }
            dt { "Published" } dd { (time(d.p)) }
            dt { "Fixed at" } dd { (time(d.fixed_at)) " " small.muted { "(the fixity time, Definition PL-4.1: H of anything built on this document)" } }
            dt { "Received" } dd { (time(d.r)) @if d.r_n != d.r && d.r_n != 0 { " (this version " (time(d.r_n)) ")" } }
            dt { "Raw ids" } dd { @for r in d.raw_ids.iter().flatten() { code { (r) } " " } }
            dt { "Language" } dd { (d.lang.clone().unwrap_or_default()) }
            dt { "Triage" } dd {
                @match triage {
                    Some(t) => { (status(Site::verdict_name(t.verdict))) " score " (crate::stats::num(t.score, 2)) " · " (t.model) " · " (crate::stats::usd(t.cost_usd)) " · " (time(t.r)) @if let Some(j) = &t.job_id { " · job " (id_link(site, root, j)) } }
                    None => { "untriaged" }
                }
            }
            dt { "Flags" } dd { @for f in feed::flags_of(d) { (status(&f)) " " } @if let Some(f) = &d.flags { small.muted { " injection " (crate::stats::num(f.injection, 2)) ", spam " (crate::stats::num(f.spam, 2)) } } }
            dt { "Enrichment" } dd { @if let Some(e) = &d.enrichment { (e.done) " done, " (e.pending) " pending, " (e.failed) " failed" } @else { "—" } }
        }
        h2 { "Text" }
        @if d.source_deleted {
            p.deleted { "source deleted" }
            p.muted { "The source removed this content; the pipeline keeps the id, the hashes and every record built on it (PL-5.5)." }
        } @else {
            div.doc-text { (marked(&d.text, &spans, root)) }
        }
        @if let Some(segs) = &d.segments {
            @if !segs.is_empty() && !d.source_deleted {
                h2 { "Segments" }
                table.data {
                    thead { tr { th { "Kind" } th { "Span" } th { "Text" } } }
                    tbody { @for s in segs { tr { td { (format!("{:?}", s.kind)) } td { (s.span_from) "–" (s.span_to) } td { (s.text) } } } }
                }
            }
        }
        @if let Some(e) = &d.entities {
            h2 { "Entities" }
            dl.facts {
                dt { "Instruments" } dd { @for i in e.instruments.iter().flatten() { code { (i.symbol) } @if let Some(c) = &i.class { " (" (c) ")" } " " small.muted { (crate::stats::num(i.confidence, 2)) } "  " } }
                dt { "Cashtags" } dd { (e.cashtags.iter().flatten().cloned().collect::<Vec<_>>().join(", ")) }
                dt { "Hashtags" } dd { (e.hashtags.iter().flatten().cloned().collect::<Vec<_>>().join(", ")) }
                dt { "Mentions" } dd { (e.mentions.iter().flatten().cloned().collect::<Vec<_>>().join(", ")) }
                dt { "URLs" } dd { @for u in e.urls.iter().flatten() { code { (u) } " " } }
            }
        }
        @if d.parents.as_ref().is_some_and(|p| !p.is_empty()) || d.links.as_ref().is_some_and(|l| !l.is_empty()) {
            h2 { "Provenance" }
            table.data {
                thead { tr { th { "Relation" } th { "Source id / URL" } th { "Document" } th { "State" } } }
                tbody {
                    @for p in d.parents.iter().flatten() {
                        tr { td { (format!("{:?}", p.kind)) } td { code { (p.source_id) } } td { @if let Some(id) = &p.doc_id { (id_link(site, root, id)) } } td { } }
                    }
                    @for l in d.links.iter().flatten() {
                        tr { td { "Link" } td { code { (l.url) } } td { @if let Some(id) = &l.doc_id { (id_link(site, root, id)) } } td { (format!("{:?}", l.state)) @if let Some(r) = &l.reason { " · " (r) } } }
                    }
                }
            }
        }
        h2 { "What this document led to" }
        @if hyps.is_empty() { p.muted { "No hypothesis yet." } }
        @else {
            ul {
                @for h in &hyps {
                    li { "Hypothesis " a href=(format!("{}{}", root, hypothesis_path(h))) title=(h) { code { (super::short(h)) } }
                        @if let Some(hy) = site.inputs.hypotheses.get(h) { ": " (hy.statement) }
                        @if let Some(s) = site.strategy_of_hypothesis.get(h) { " → strategy " a href=(format!("{}{}", root, strategy_path(s))) title=(s) { code { (super::short(s)) } } }
                    }
                }
            }
        }
        @if !strategies.is_empty() {
            p { "Strategies from this post: " @for s in &strategies { a href=(format!("{}{}", root, strategy_path(s))) title=(s) { code { (super::short(s)) } } " " } }
        }
        p { a href=(format!("{}feed.html#doc-{}", root, d.id)) { "This document in the feed" } " · " a href=(format!("{}{}", root, document_path(&d.id))) { "permalink" } }
    };
    frame(site, root, &format!("Document {}", super::short(&d.id)), body)
}

/// The text with the cited spans marked and linked to their hypotheses;
/// spans are character offsets, overlapping ones are merged.
fn marked(text: &str, spans: &[(u32, u32, String)], root: &str) -> Markup {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut pos = 0usize;
    for (from, to, h) in spans {
        let (from, to) = ((*from as usize).min(chars.len()), (*to as usize).min(chars.len()));
        if from < pos || from >= to {
            continue;
        }
        let before: String = chars[pos..from].iter().collect();
        let inside: String = chars[from..to].iter().collect();
        out.push(html! { (before) a href=(format!("{}{}", root, hypothesis_path(h))) title=(format!("evidence of {}", h)) { mark { (inside) } } });
        pos = to;
    }
    let rest: String = chars[pos..].iter().collect();
    html! { @for m in out { (m) } (rest) }
}

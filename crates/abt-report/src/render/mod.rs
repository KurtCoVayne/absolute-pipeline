//! The pages (PL-7.4), as `maud` templates: static HTML a person reads
//! without JavaScript, one stylesheet, a few lines of script for the
//! feed's filters. Every page carries the build time and the inputs'
//! offsets; every id is a link to what it names (GP-6's walk: a strategy
//! to its post and the post to its strategy).

pub mod account;
pub mod document;
pub mod feed;
pub mod hypothesis;
pub mod leaderboard;
pub mod operations;
pub mod strategy;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use maud::{html, Markup, PreEscaped, DOCTYPE};
use pipeline_wire::{is_id, Time};

use crate::site::Site;
use crate::stats::num;
use crate::Table;

pub const STYLE: &str = include_str!("style.css");

/// The pages' paths, relative to the output directory.
pub fn strategy_path(id: &str) -> String {
    format!("strategies/{}.html", id)
}
pub fn hypothesis_path(id: &str) -> String {
    format!("hypotheses/{}.html", id)
}
pub fn document_path(id: &str) -> String {
    format!("documents/{}.html", id)
}
pub fn account_path(handle: &str) -> String {
    format!("accounts/{}.html", safe(handle))
}

/// A handle as a file name: X handles are `[A-Za-z0-9_]`, anything else is
/// replaced so no path escapes the directory.
pub fn safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

/// The first twelve hex digits, enough to tell ids apart on a page.
pub fn short(id: &str) -> &str {
    if id.len() > 12 {
        &id[..12]
    } else {
        id
    }
}

/// An id as a link to its page when the site has one.
pub fn id_link(site: &Site, root: &str, id: &str) -> Markup {
    let target = if site.inputs.documents.contains_key(id) {
        Some(document_path(id))
    } else if site.inputs.hypotheses.contains_key(id) {
        Some(hypothesis_path(id))
    } else if site.inputs.submissions.contains_key(id) {
        Some(strategy_path(id))
    } else if let Some(s) = site.strategy_of_plan.get(id) {
        Some(format!("{}#plan-{}", strategy_path(s), id))
    } else if site.inputs.dlq.iter().any(|j| j.id == id) {
        Some(format!("operations.html#job-{}", id))
    } else {
        None
    };
    match target {
        Some(t) => html! { a.id href=(format!("{}{}", root, t)) title=(id) { code { (short(id)) } } },
        None if is_id(id) => html! { code.id title=(id) { (short(id)) } },
        None => html! { code { (id) } },
    }
}

pub fn time(t: Time) -> Markup {
    if t == 0 {
        return html! { "—" };
    }
    let s = pipeline_wire::iso(t);
    html! { time datetime=(s) { (s) } }
}

pub fn opt_num(v: Option<f64>, digits: usize) -> String {
    v.map(|v| num(v, digits)).unwrap_or_else(|| "—".into())
}

/// A report table as HTML, every column, cells formatted by their type.
pub fn table_markup(t: &Table) -> Markup {
    html! {
        table.data {
            thead { tr { @for c in &t.columns { th { (c) } } } }
            tbody {
                @for row in &t.rows {
                    tr { @for v in row { td { (cell(v)) } } }
                }
            }
        }
    }
}

pub fn cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "—".into(),
        serde_json::Value::Number(n) => n.as_f64().map(|f| if f.fract() == 0.0 { num(f, 0) } else { num(f, 3) }).unwrap_or_else(|| n.to_string()),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

/// The page frame: the navigation, the body, the build's footer.
pub fn frame(site: &Site, root: &str, title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " · absolute-pipeline" }
                link rel="stylesheet" href=(format!("{}style.css", root));
            }
            body {
                header {
                    nav {
                        a href=(format!("{}index.html", root)) { strong { "absolute-pipeline" } }
                        a href=(format!("{}feed.html", root)) { "Feed" }
                        a href=(format!("{}leaderboard.html", root)) { "Leaderboard" }
                        a href=(format!("{}accounts.html", root)) { "Accounts" }
                        a href=(format!("{}operations.html", root)) { "Operations" }
                    }
                }
                main { (body) }
                footer {
                    p { "Built " (time(site.built_at)) ". Nothing here trades; the pipeline ends at evidence." }
                    details {
                        summary { "Inputs: " (site.inputs.offsets.len()) " sources, " (site.inputs.offsets.iter().map(|o| o.records).sum::<u64>()) " records" }
                        table.data.offsets {
                            thead { tr { th { "Input" } th { "Read up to" } th { "Records" } } }
                            tbody {
                                @for o in &site.inputs.offsets {
                                    tr { td { code { (o.input) } } td { (o.detail) } td { (o.records) } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A status word as a class for the stylesheet's colours.
pub fn status_class(s: &str) -> &'static str {
    let s = s.to_lowercase();
    if s.starts_with("holds") || s.starts_with("proven") || s == "checked" || s == "guaranteed" || s == "data-guaranteed" || s == "done" || s == "active" {
        "ok"
    } else if s.starts_with("fails") || s.starts_with("violated") || s == "failed" || s == "poisoned" || s == "retired" || s == "open" {
        "bad"
    } else if s.starts_with("undecidable") || s.starts_with("warned") || s == "warned" || s == "modeled" || s == "measured" {
        "warn"
    } else {
        "plain"
    }
}

pub fn status(s: &str) -> Markup {
    html! { span.status.(status_class(s)) { (s) } }
}

fn write(out: &Path, rel: &str, markup: Markup) -> Result<PathBuf> {
    let path = out.join(rel);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&path, markup.into_string()).with_context(|| path.display().to_string())?;
    Ok(PathBuf::from(rel))
}

/// Writes every page; the paths written, relative to `out`.
pub fn write_all(site: &Site, out: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(out)?;
    let mut pages = Vec::new();
    std::fs::write(out.join("style.css"), STYLE)?;
    pages.push(PathBuf::from("style.css"));
    pages.push(write(out, "index.html", index(site))?);
    pages.push(write(out, "feed.html", feed::page(site))?);
    pages.push(write(out, "leaderboard.html", leaderboard::page(site))?);
    pages.push(write(out, "accounts.html", account::index(site))?);
    pages.push(write(out, "operations.html", operations::page(site))?);
    for a in site.accounts.values() {
        pages.push(write(out, &account_path(&a.handle), account::page(site, a))?);
    }
    for h in site.inputs.hypotheses.values() {
        pages.push(write(out, &hypothesis_path(&h.id), hypothesis::page(site, h))?);
    }
    for s in &site.strategies {
        pages.push(write(out, &strategy_path(s.id()), strategy::page(site, s))?);
    }
    for d in site.inputs.documents.values() {
        pages.push(write(out, &document_path(&d.id), document::page(site, d))?);
    }
    Ok(pages)
}

/// The front page: what the site holds and where to start.
pub fn index(site: &Site) -> Markup {
    let i = site.inputs;
    let forward = site.leaderboard.len();
    let body = html! {
        h1 { "The pipeline's evidence" }
        p.lede { "A growing population of time-stamped, pre-registered strategies with forward evidence, each counted against its lineage's trials. The leaderboard orders by forward evidence and never by in-sample Sharpe." }
        div.cards {
            a.card href="feed.html" { strong { (i.documents.len()) } " documents" br; small { (i.triage.len()) " triage verdicts" } }
            a.card href="leaderboard.html" { strong { (forward) } " with forward evidence" br; small { (site.insample_only.len()) " in-sample only" } }
            a.card href="accounts.html" { strong { (site.accounts.len()) } " accounts" br; small { (i.hypotheses.len()) " hypotheses" } }
            a.card href="operations.html" { strong { (i.submissions.len()) } " submissions" br; small { (i.outcomes.len()) " reveals" } }
        }
        h2 { "Leading by forward evidence" }
        (leaderboard::forward_table(site, "", &site.leaderboard.iter().take(10).cloned().collect::<Vec<_>>()))
        p { a href="leaderboard.html" { "The full leaderboard" } }
    };
    frame(site, "", "Front", body)
}

pub fn raw(s: &str) -> PreEscaped<String> {
    PreEscaped(s.to_string())
}

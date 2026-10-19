//! A strategy's page (PL-7.4): the provenance chain post → hypothesis →
//! submission → outcomes; the program and plan texts; the contract
//! statuses; the five-part conclusion; the Full / in-sample /
//! out-of-sample (/ forward) table; the author comparison; the red-flag
//! rows with the ones that fire marked; the contribution by market and by
//! year; the sensitivity surface as a heat table; the forward track; the
//! bias table; the lineage's trial count by kind.

use maud::{html, Markup};
use serde_json::Value;

use super::{account_path, cell, document_path, frame, hypothesis_path, id_link, status, table_markup, time};
use crate::flags::red_flags;
use crate::site::{Site, Strategy};
use crate::stats::{num, pct, usd};
use crate::Table;

const METRICS: &[(&str, &str, usize)] = &[
    ("return", "Return % a year", 2),
    ("volatility", "Volatility %", 2),
    ("sharpe", "Sharpe", 2),
    ("mar", "MAR", 2),
    ("max_drawdown", "Max drawdown", 1),
    ("drawdown_duration", "Drawdown years", 1),
    ("trades", "Trades", 0),
    ("profit_factor", "Profit factor", 2),
    ("hit_rate", "% winners", 1),
    ("avg_win_loss", "Avg win / loss", 2),
];

pub fn page(site: &Site, s: &Strategy) -> Markup {
    let root = "../";
    let sub = s.submission;
    let flags = red_flags(s);
    let body = html! {
        h1 { "Strategy " code { (super::short(&sub.id)) } }
        @if let Some(h) = s.hypothesis { p.lede { (h.statement) } }
        (chain(s, root))
        dl.facts {
            dt { "Submission" } dd { code { (sub.id) } " · " (status(&s.status())) }
            dt { "H" } dd { (time(sub.h)) " " small.muted { "(in-sample before, forward after)" } }
            dt { "Submitted" } dd { (time(sub.r)) " · week " (crate::stats::iso_week(sub.r)) }
            dt { "Program" } dd { code { (sub.program_hash) } }
            dt { "Plan" } dd { code { (sub.plan_hash) } }
            dt { "Forward plan" } dd { @if let Some(f) = &sub.forward_plan_hash { code { (f) } } @else { "—" } }
            dt { "Lineage" } dd { code { (sub.lineage) } " · " (sub.trials_used) " trials used by this job" }
            dt { "Agent" } dd { (sub.agent) " · " (sub.model) @if let Some(c) = &sub.campaign { " · campaign " (c) } @if let Some(k) = &sub.skills_hash { " · skills " code { (super::short(k)) } } }
            dt { "Cost" } dd { (usd(sub.cost_usd)) @if let Some(j) = &sub.job_id { " · job " (id_link(site, root, j)) } }
        }

        h2 id="conclusion" { "Conclusion" }
        (conclusion(&sub.rationale))

        h2 id="numbers" { "The numbers that matter" }
        (windows_table(s))
        @if let Some(v) = s.insample().get("verdicts") { h3 { "In-sample verdicts at submission" } (verdicts_list(v)) }

        h2 id="author" { "The author's numbers beside ours" }
        (author_comparison(s))

        h2 id="flags" { "Red flags" }
        p.muted { "The guideline's rows (§12), computed from the report tables; a row that fires is marked." }
        table.data.flags {
            thead { tr { th { "Flag" } th { "Threshold" } th { "Found" } th { "Fires" } th { "Source" } } }
            tbody {
                @for f in &flags {
                    tr.fires[f.fires == Some(true)] {
                        td { (f.name) } td { (f.threshold) } td { (f.value) }
                        td { @match f.fires { Some(true) => (status("fires")), Some(false) => (status("clear")), None => (status("not computed")) } }
                        td { (f.source) }
                    }
                }
            }
        }

        h2 id="contribution" { "Where the P&L comes from" }
        (contribution(s))

        h2 id="surface" { "Sensitivity" }
        (surface(s))

        h2 id="forward" { "The forward track" }
        (forward_track(s))

        h2 id="claims" { "Claims" }
        @match s.table("claims") { Some(t) => (claims_table(t)), None => p.muted { "No claims table." } }

        h2 id="contract" { "Contract" }
        @match s.table("contract") { Some(t) => (status_table(t, "status")), None => p.muted { "No contract table." } }

        h2 id="biases" { "Biases" }
        @match s.table("biases") { Some(t) => (status_table(t, "status")), None => p.muted { "No bias table." } }

        h2 id="trials" { "The lineage's trials" }
        @match s.table("trials") { Some(t) => (table_markup(t)), None => p.muted { "No trials table." } }
        @if let Some(t) = s.table("warnings") { h3 { "Warnings" } (table_markup(t)) }
        @if let Some(t) = s.table("folds") { h3 { "Walk-forward folds" } (table_markup(t)) }

        h2 id=(format!("plan-{}", sub.plan_hash)) { "The program" }
        pre { (sub.program_text) }
        h2 { "The plan" }
        pre { (sub.plan_text) }
        @if let Some(f) = &sub.forward_plan_text {
            h2 id=(format!("plan-{}", sub.forward_plan_hash.clone().unwrap_or_default())) { "The forward plan" }
            pre { (f) }
        }
        h2 { "Pre-registered claims" }
        pre { @for c in sub.claims.iter().flatten() { (c) "\n" } }
    };
    frame(site, root, &format!("Strategy {}", super::short(&sub.id)), body)
}

/// The provenance chain: post → hypothesis → submission → outcomes.
fn chain(s: &Strategy, root: &str) -> Markup {
    html! {
        div.chain {
            @for d in &s.documents {
                span { "post " a href=(format!("{}{}", root, document_path(&d.id))) title=(d.id) { code { (super::short(&d.id)) } } " by " a href=(format!("{}{}", root, account_path(&d.author.handle))) { "@" (d.author.handle) } @if d.source_deleted { " " span.deleted { "source deleted" } } }
            }
            @if s.documents.is_empty() { span.muted { "(no document on the log)" } }
            span.arrow { "→" }
            span { "hypothesis " a href=(format!("{}{}", root, hypothesis_path(&s.submission.hypothesis_id))) title=(s.submission.hypothesis_id) { code { (super::short(&s.submission.hypothesis_id)) } } }
            span.arrow { "→" }
            span { "submission " code { (super::short(&s.submission.id)) } }
            span.arrow { "→" }
            @if s.outcomes.is_empty() { span.muted { "no reveal yet" } }
            @for o in &s.outcomes { span { "reveal " (o.reveal) " " code title=(o.id) { (super::short(&o.id)) } } }
        }
    }
}

/// The rationale's five parts (guideline §13), split on its numbered headings.
fn conclusion(rationale: &str) -> Markup {
    let parts = split_parts(rationale);
    if parts.len() < 2 {
        return html! { div.conclusion { pre { (rationale) } } };
    }
    html! {
        div.conclusion {
            ol {
                @for (title, text) in &parts {
                    li { strong { (title) } (maud::PreEscaped(" ")) div { (paragraphs(text)) } }
                }
            }
        }
    }
}

/// `1. **Title.** text` or `1. Title: text` blocks → (title, text).
fn split_parts(s: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in s.lines() {
        let t = line.trim();
        let numbered = t.len() > 2 && t.as_bytes()[0].is_ascii_digit() && (t.as_bytes()[1] == b'.' || t.as_bytes()[1] == b')');
        if numbered {
            let rest = t[2..].trim();
            let (title, body) = match rest.strip_prefix("**").and_then(|r| r.split_once("**")) {
                Some((title, body)) => (title.trim().to_string(), body.trim().to_string()),
                None => match rest.split_once(':') {
                    Some((title, body)) if title.len() < 60 => (title.trim().to_string(), body.trim().to_string()),
                    _ => (String::new(), rest.to_string()),
                },
            };
            out.push((title, body));
        } else if let Some(last) = out.last_mut() {
            last.1.push('\n');
            last.1.push_str(line);
        } else if !t.is_empty() {
            out.push((String::new(), line.to_string()));
        }
    }
    out
}

fn paragraphs(text: &str) -> Markup {
    let blocks: Vec<&str> = text.split("\n\n").map(str::trim).filter(|b| !b.is_empty()).collect();
    html! {
        @for b in blocks {
            @if b.starts_with('|') { (markdown_table(b)) } @else { p { (b) } }
        }
    }
}

/// A Markdown pipe table in a rationale, as a table.
fn markdown_table(b: &str) -> Markup {
    let rows: Vec<Vec<String>> = b
        .lines()
        .filter(|l| !l.trim().chars().all(|c| c == '|' || c == '-' || c == ':' || c == ' '))
        .map(|l| l.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect())
        .collect();
    html! {
        table.data {
            @if let Some(h) = rows.first() { thead { tr { @for c in h { th { (c) } } } } }
            tbody { @for r in rows.iter().skip(1) { tr { @for c in r { td { (c) } } } } }
        }
    }
}

/// Full / in-sample / out-of-sample (/ forward) for the `base` run.
fn windows_table(s: &Strategy) -> Markup {
    let Some(t) = s.summary() else { return html! { p.muted { "No summary table for the plan." } } };
    let mut cols: Vec<(&str, &Table, usize)> = Vec::new();
    for (w, label) in [("full", "Full"), ("insample", "In-sample"), ("oos", "Out-of-sample")] {
        if let Some(i) = s.summary_row("base", w) {
            cols.push((label, t, i));
        }
    }
    if let Some((ft, i)) = s.forward_row() {
        cols.push(("Forward", ft, i));
    }
    html! {
        table.data {
            thead { tr { th { "Metric" } @for (label, t, i) in &cols { th.num { (label) @if let Some(y) = t.f64(*i, "years") { br; small.muted { (num(y, 1)) " y" } } } } } }
            tbody {
                @for (key, label, digits) in METRICS {
                    tr { td { (label) } @for (_, t, i) in &cols { td.num { (metric(t, *i, key, *digits)) } } }
                }
                tr { td { "DSR (lineage count)" } @for (_, t, i) in &cols { td.num { (super::opt_num(t.f64(*i, "dsr"), 3)) } } }
                tr { td { "Trials counted" } @for (_, t, i) in &cols { td.num { (super::opt_num(t.f64(*i, "trials"), 0)) } } }
            }
        }
        @if cols.len() > 1 { p { (oos_sentence(s)) } }
    }
}

fn metric(t: &Table, i: usize, key: &str, digits: usize) -> String {
    match (key, t.f64(i, key)) {
        (_, None) => "—".into(),
        ("max_drawdown" | "hit_rate", Some(v)) => pct(v),
        (_, Some(v)) => num(v, digits),
    }
}

fn oos_sentence(s: &Strategy) -> String {
    let (Some(i), Some(o)) = (s.summary_row("base", "insample"), s.summary_row("base", "oos")) else {
        return String::new();
    };
    let t = s.summary().expect("rows imply the table");
    let (si, so) = (t.f64(i, "sharpe").unwrap_or(0.0), t.f64(o, "sharpe").unwrap_or(0.0));
    if so <= 0.0 {
        format!("Out of sample the Sharpe is {} against {} in-sample: the edge did not survive the hold-out.", num(so, 2), num(si, 2))
    } else if so < si * 0.5 {
        format!(
            "Out of sample the Sharpe is {} against {} in-sample: less than half of the in-sample edge survived.",
            num(so, 2),
            num(si, 2)
        )
    } else {
        format!("Out of sample the Sharpe is {} against {} in-sample: the edge held on the hold-out.", num(so, 2), num(si, 2))
    }
}

fn verdicts_list(v: &Value) -> Markup {
    match v.as_object() {
        Some(m) => html! { ul { @for (k, v) in m { li { code { (k) } ": " (status(&cell(v))) } } } },
        None => html! { pre { (v.to_string()) } },
    }
}

/// The author's reported numbers beside the full-window `base` row.
fn author_comparison(s: &Strategy) -> Markup {
    let Some(a) = s.author_reported() else {
        return html! { p.muted { "The analyst recorded no numbers from the author." } };
    };
    let full = s.summary_row("base", "full");
    let t = s.summary();
    let ours = |key: &str| -> String {
        full.and_then(|i| t.and_then(|t| t.f64(i, key)))
            .map(|v| if key == "max_drawdown" { pct(v) } else { num(v, 2) })
            .unwrap_or_else(|| "—".into())
    };
    let theirs = |key: &str| -> String { a.get(key).map(cell).unwrap_or_else(|| "—".into()) };
    html! {
        table.data {
            thead { tr { th { "" } th { "The author" } th { "Ours (full window, base)" } } }
            tbody {
                tr { td { "Return % a year" } td { (theirs("net_pct")) } td { (ours("return")) } }
                tr { td { "Max drawdown" } td { (theirs("drawdown_pct")) } td { (ours("max_drawdown")) } }
                tr { td { "Sharpe" } td { (theirs("sharpe")) } td { (ours("sharpe")) } }
                tr { td { "Trades" } td { (theirs("trades")) } td { (ours("trades")) } }
                tr { td { "Capital" } td { (theirs("capital")) } td { "1,000,000 USD fixed, no compounding" } }
                tr { td { "Costs" } td { (theirs("costs")) } td { "7.50 USD round turn per contract" } }
                tr { td { "Sizing" } td { (theirs("sizing")) } td { "the house preset" } }
                tr { td { "Instrument" } td { (theirs("instrument")) } td { (s.hypothesis.map(|h| h.universe.clone()).unwrap_or_default()) } }
                tr { td { "Dates" } td { (theirs("from")) " – " (theirs("to")) } td { (s.documents.first().map(|_| format!("window to H = {}", crate::stats::date(s.submission.h))).unwrap_or_default()) } }
            }
        }
    }
}

/// Contribution by market (the `contribution` table) and by year (the
/// `subperiods` table).
fn contribution(s: &Strategy) -> Markup {
    let c = s.table("contribution");
    let y = s.table("subperiods");
    html! {
        div.two {
            div {
                h3 { "By market" }
                @match c {
                    Some(c) => {
                        table.data {
                            thead { tr { th { "Kind" } th { "Entity" } th.num { "Net profit" } th.num { "Share" } th.num { "Trades" } } }
                            tbody {
                                @for i in 0..c.len() {
                                    tr.fires[c.str(i, "kind") == Some("market") && c.f64(i, "share").unwrap_or(0.0) > 0.4] {
                                        td { (c.text(i, "kind")) } td { (c.text(i, "entity")) }
                                        td.num { (c.f64(i, "net_profit").map(usd).unwrap_or_default()) }
                                        td.num { (c.f64(i, "share").map(pct).unwrap_or_default()) }
                                        td.num { (super::opt_num(c.f64(i, "trades"), 0)) }
                                    }
                                }
                            }
                        }
                    }
                    None => p.muted { "No contribution table." }
                }
            }
            div {
                h3 { "By year" }
                @match y {
                    Some(y) => {
                        table.data {
                            thead { tr { th { "Period" } th.num { "Return %" } th.num { "Sharpe" } th.num { "Max DD" } th.num { "Trades" } th.num { "Net profit" } } }
                            tbody {
                                @for i in 0..y.len() {
                                    tr {
                                        td { (y.text(i, "period")) }
                                        td.num { (super::opt_num(y.f64(i, "return"), 1)) }
                                        td.num { (super::opt_num(y.f64(i, "sharpe"), 2)) }
                                        td.num { (y.f64(i, "max_drawdown").map(pct).unwrap_or_default()) }
                                        td.num { (super::opt_num(y.f64(i, "trades"), 0)) }
                                        td.num { (y.f64(i, "net_profit").map(usd).unwrap_or_default()) }
                                    }
                                }
                            }
                        }
                    }
                    None => p.muted { "No sub-period table." }
                }
            }
        }
    }
}

/// The parameter surface as a heat table: rows by the first parameter,
/// columns by the second, cells the objective, the best cell outlined.
fn surface(s: &Strategy) -> Markup {
    let Some(t) = s.table("surface") else { return html! { p.muted { "No surface table." } } };
    if t.columns.len() < 3 {
        return table_markup(t);
    }
    let obj = t.col("sharpe").unwrap_or(t.columns.len() - 1);
    let (p1, p2) = (t.columns[0].clone(), t.columns[1].clone());
    let mut xs: Vec<f64> = (0..t.len()).filter_map(|i| t.f64(i, &p1)).collect();
    let mut ys: Vec<f64> = (0..t.len()).filter_map(|i| t.f64(i, &p2)).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ys.dedup();
    let value = |x: f64, y: f64| (0..t.len()).find(|&i| t.f64(i, &p1) == Some(x) && t.f64(i, &p2) == Some(y)).and_then(|i| t.f64(i, &t.columns[obj]));
    let all: Vec<f64> = (0..t.len()).filter_map(|i| t.f64(i, &t.columns[obj])).collect();
    let best = all.iter().cloned().fold(f64::MIN, f64::max);
    let worst = all.iter().cloned().fold(f64::MAX, f64::min);
    let bucket = |v: f64| -> usize {
        if best <= worst {
            3
        } else {
            (((v - worst) / (best - worst)) * 5.0).round().clamp(0.0, 5.0) as usize
        }
    };
    let share = crate::flags::neighbour_share(t);
    html! {
        p { "Objective " code { (t.columns[obj]) } " over " code { (p1) } " (rows) × " code { (p2) } " (columns); the best cell is outlined."
            @if let Some((sh, n)) = share { " " (pct(sh)) " of its " (n) " neighbours are within 10 % of it: " @if sh < 0.5 { strong { "a cliff" } } @else { "a plateau" } "." } }
        table.data.heat {
            thead { tr { th { (p1) " \\ " (p2) } @for y in &ys { th.num { (crate::stats::param(*y)) } } } }
            tbody {
                @for x in &xs {
                    tr { th { (crate::stats::param(*x)) }
                        @for y in &ys {
                            @match value(*x, *y) {
                                Some(v) => td.cell.(format!("h{}", bucket(v))).best[v == best] { (num(v, 2)) },
                                None => td.cell { "—" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Each reveal: its window, the forward metrics, the verdict pairs, the
/// forward DSR and the minimum track-record length progress.
fn forward_track(s: &Strategy) -> Markup {
    if s.outcomes.is_empty() {
        return html! { p.muted { "No reveal yet: the forward window (H, now] is revealed by the scheduler, never by an agent (PL-7.3). This submission is in-sample only." } };
    }
    html! {
        @for o in &s.outcomes {
            @let m = Strategy::outcome_metrics(o);
            @let fwd = m.get("forward").cloned().unwrap_or(Value::Null);
            @let dsr = m.get("dsr").and_then(Value::as_f64);
            @let track = m.get("min_track_record").cloned().unwrap_or(Value::Null);
            @let observed = track.get("observed").and_then(Value::as_f64).unwrap_or(0.0);
            @let required = track.get("required").and_then(Value::as_f64).unwrap_or(0.0);
            @let unit = track.get("unit").and_then(Value::as_str).unwrap_or("periods");
            h3 { "Reveal " (o.reveal) " · " (status(&format!("{:?}", o.status).to_lowercase())) " · " code title=(o.id) { (super::short(&o.id)) } }
            dl.facts {
                dt { "Window" } dd { (time(o.forward_from)) " → " (time(o.forward_to)) }
                dt { "Revealed" } dd { (time(o.r)) }
                dt { "Forward DSR" } dd { (super::opt_num(dsr, 3)) @if let Some(p) = m.get("psr").and_then(Value::as_f64) { " · PSR " (num(p, 3)) } }
                dt { "Track record" } dd {
                    (num(observed, 0)) " of " (num(required, 0)) " " (unit) " required "
                    span.progress { span style=(format!("width: {}%", if required > 0.0 { (observed / required * 100.0).min(100.0) } else { 0.0 })) {} }
                    @if required > 0.0 && observed >= required { " " (status("long enough")) } @else { " " (status("too short yet")) }
                }
                @if let Some(r) = &o.retired_reason { dt { "Retired" } dd { (r) } }
            }
            @if let Some(f) = fwd.as_object() {
                table.data {
                    thead { tr { @for (key, label, _) in METRICS { @if f.contains_key(*key) { th.num { (label) } } } } }
                    tbody { tr { @for (key, _, digits) in METRICS { @if let Some(v) = f.get(*key) { td.num { (v.as_f64().map(|v| if *key == "max_drawdown" || *key == "hit_rate" { pct(v) } else { num(v, *digits) }).unwrap_or_default()) } } } } }
                }
            }
            @let pairs = Strategy::pairs(o);
            @if !pairs.is_empty() {
                table.data {
                    thead { tr { th { "Claim" } th { "In-sample" } th { "Forward" } th.num { "p" } } }
                    tbody { @for (c, i, f, p) in &pairs { tr { td { code { (c) } } td { (status(i)) } td { (status(f)) } td.num { (super::opt_num(*p, 4)) } } } }
                }
            }
            @if let Some(v) = &o.verdicts { @if !v.is_empty() { p.muted { "Verdicts as recorded: " @for x in v { code { (x) } " " } } } }
            @if let Some(t) = m.get("trials").and_then(Value::as_object) {
                p.muted { "Lineage trials at this reveal: " @for (k, v) in t { (k) " " (cell(v)) "; " } }
            }
        }
    }
}

fn claims_table(t: &Table) -> Markup {
    html! {
        table.data {
            thead { tr { @for c in &t.columns { th { (c) } } } }
            tbody {
                @for i in 0..t.len() {
                    tr { @for (j, c) in t.columns.iter().enumerate() {
                        @let v = cell(&t.rows[i][j]);
                        td { @if c == "insample" || c == "oos" || c == "forward" || c == "verdict" { (status(&v)) } @else { (v) } }
                    } }
                }
            }
        }
    }
}

/// A table whose `status_col` is rendered as a status.
fn status_table(t: &Table, status_col: &str) -> Markup {
    html! {
        table.data {
            thead { tr { @for c in &t.columns { th { (c) } } } }
            tbody {
                @for i in 0..t.len() {
                    tr { @for (j, c) in t.columns.iter().enumerate() {
                        @let v = cell(&t.rows[i][j]);
                        td { @if c == status_col { (status(&v)) } @else { (v) } }
                    } }
                }
            }
        }
    }
}

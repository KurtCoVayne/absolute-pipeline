//! The site from the fixtures (contracts.md section 8, PL-7.4): every page
//! exists, the leaderboard orders by forward evidence, the walk from a
//! strategy to its post and back (GP-6), the red flags fire on the fixture
//! built for them, the cohort's BH adjustment, well-formed HTML whose links
//! resolve inside the output directory.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use abt_report::flags::red_flags;
use abt_report::site::Site;
use abt_report::stats::benjamini_hochberg;
use abt_report::{build, fixtures, Inputs};
use scraper::{Html, Selector};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn load() -> Inputs {
    let mut i = fixtures::load(&fixtures_dir()).expect("the fixtures load");
    i.facts = Some(serde_json::json!({"instance": "test"}));
    i
}

fn built() -> (tempfile::TempDir, Inputs) {
    let dir = tempfile::tempdir().unwrap();
    let inputs = load();
    build(&inputs, dir.path()).expect("the site builds");
    (dir, inputs)
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("{}: {}", rel, e))
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).unwrap()
}

#[test]
fn the_fixtures_are_what_the_plan_asks_for() {
    let i = load();
    assert_eq!(i.documents.len(), 40);
    assert_eq!(i.triage.len(), 40);
    assert_eq!(i.hypotheses.len(), 12);
    assert_eq!(i.submissions.len(), 8);
    assert_eq!(i.outcomes.len(), 6);
    assert_eq!(i.outcomes.iter().map(|o| o.reveal).max(), Some(2));
    assert_eq!(i.follows.len(), 10);
    assert!(i.budgets.len() >= 4);
    assert_eq!(i.tombstones.len(), 1);
    let accounts: BTreeSet<&str> = i.documents.values().map(|d| d.author.handle.as_str()).collect();
    assert_eq!(accounts.len(), 10);
    for s in i.submissions.values() {
        assert!(
            s.h >= pipeline_wire::parse_iso("2026-09-01").unwrap() && s.h < pipeline_wire::parse_iso("2026-10-01").unwrap(),
            "H in September 2026"
        );
        let tables = &i.reports[&s.plan_hash];
        for t in [
            "summary",
            "trials",
            "claims",
            "contract",
            "warnings",
            "biases",
            "surface",
            "folds",
            "subperiods",
            "contribution",
            "trades",
        ] {
            assert!(tables.contains_key(t), "{} has {}", s.plan_hash, t);
        }
        assert_eq!(tables["biases"].len(), 40);
        for m in [
            "return",
            "volatility",
            "sharpe",
            "mar",
            "max_drawdown",
            "drawdown_duration",
            "trades",
            "profit_factor",
            "hit_rate",
            "avg_win_loss",
        ] {
            assert!(tables["summary"].col(m).is_some(), "summary has {}", m);
        }
        assert!(s.rationale.contains("1. **What was tested.**") && s.rationale.contains("5. **Verdict.**"));
    }
}

#[test]
fn the_fixtures_are_the_generator_output() {
    let dir = tempfile::tempdir().unwrap();
    abt_report::sample::write(dir.path()).unwrap();
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_path_buf());
            }
        }
    }
    let mut generated = Vec::new();
    walk(dir.path(), dir.path(), &mut generated);
    let mut committed = Vec::new();
    walk(&fixtures_dir(), &fixtures_dir(), &mut committed);
    generated.sort();
    committed.sort();
    assert_eq!(generated, committed, "the file lists differ: run `abt-report fixtures --out crates/abt-report/fixtures`");
    for rel in &generated {
        assert_eq!(
            std::fs::read(dir.path().join(rel)).unwrap(),
            std::fs::read(fixtures_dir().join(rel)).unwrap(),
            "{} differs: run `abt-report fixtures --out crates/abt-report/fixtures`",
            rel.display()
        );
    }
}

#[test]
fn every_page_of_the_plan_exists() {
    let (dir, inputs) = built();
    for p in ["index.html", "feed.html", "leaderboard.html", "accounts.html", "operations.html", "style.css"] {
        assert!(dir.path().join(p).exists(), "{}", p);
    }
    for s in inputs.submissions.values() {
        assert!(dir.path().join(format!("strategies/{}.html", s.id)).exists(), "strategy {}", s.id);
    }
    for h in inputs.hypotheses.values() {
        assert!(dir.path().join(format!("hypotheses/{}.html", h.id)).exists());
    }
    for d in inputs.documents.values() {
        assert!(dir.path().join(format!("documents/{}.html", d.id)).exists());
    }
    let site = Site::new(&inputs, 0);
    for a in site.accounts.keys() {
        assert!(dir.path().join(format!("accounts/{}.html", a)).exists(), "account {}", a);
    }
    let ops = read(dir.path(), "operations.html");
    assert!(ops.contains("stage:triage") && ops.contains("Dead-letter") && ops.contains("not_testable") && ops.contains("Compliance") && ops.contains("instance"));
    let feed = read(dir.path(), "feed.html");
    assert!(feed.contains("data-verdict=\"candidate\"") && feed.contains("<select"));
}

#[test]
fn the_leaderboard_orders_by_forward_dsr_then_track_length_never_by_insample_sharpe() {
    let inputs = load();
    let site = Site::new(&inputs, 0);
    assert_eq!(site.leaderboard.len(), 4);
    assert_eq!(site.insample_only.len(), 4);
    for w in site.leaderboard.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let da = a.dsr.unwrap();
        let db = b.dsr.unwrap();
        assert!(da > db || (da == db && a.track_observed >= b.track_observed), "{:?} before {:?}", a, b);
    }
    // The submission with an in-sample Sharpe of 5 has no forward evidence: it is in the lower list.
    let five = site.insample_only.iter().find(|r| r.insample_sharpe == Some(5.0)).expect("the in-sample Sharpe 5 fixture");
    assert!(!site.leaderboard.iter().any(|r| r.submission_id == five.submission_id));
    // Within the forward list, the in-sample Sharpe 3.5 submission sits below one with a higher forward DSR and a lower in-sample Sharpe.
    let pos35 = site.leaderboard.iter().position(|r| r.insample_sharpe == Some(3.5)).expect("the in-sample Sharpe 3.5 fixture");
    assert!(pos35 > 0);
    assert!(site.leaderboard[0].insample_sharpe.unwrap() < 3.5);
    assert!(site.leaderboard[0].dsr > site.leaderboard[pos35].dsr);
    // An in-sample Sharpe of 5 given to a forward submission with a low DSR moves it nowhere.
    let mut inputs2 = load();
    let last = site.leaderboard.last().unwrap().submission_id.clone();
    let plan = inputs2.submissions[&last].plan_hash.clone();
    let t = inputs2.reports.get_mut(&plan).unwrap().get_mut("summary").unwrap();
    let row = (0..t.len()).find(|&i| t.str(i, "run") == Some("base") && t.str(i, "window") == Some("insample")).unwrap();
    let col = t.col("sharpe").unwrap();
    t.rows[row][col] = serde_json::json!(5.0);
    let site2 = Site::new(&inputs2, 0);
    assert_eq!(site2.leaderboard.last().unwrap().submission_id, last);
    assert_eq!(site2.leaderboard.last().unwrap().insample_sharpe, Some(5.0));
    let order: Vec<&String> = site.leaderboard.iter().map(|r| &r.submission_id).collect();
    let order2: Vec<&String> = site2.leaderboard.iter().map(|r| &r.submission_id).collect();
    assert_eq!(order, order2);
}

#[test]
fn the_cohort_adjustment_is_benjamini_hochberg_per_week() {
    let inputs = load();
    let site = Site::new(&inputs, 0);
    let weeks: BTreeSet<&str> = site.leaderboard.iter().map(|r| r.week.as_str()).collect();
    for w in weeks {
        let rows: Vec<_> = site.leaderboard.iter().filter(|r| r.week == w).collect();
        let p: Vec<f64> = rows.iter().map(|r| r.p_value.unwrap()).collect();
        let q = benjamini_hochberg(&p);
        for (r, q) in rows.iter().zip(q) {
            assert_eq!(r.q_value, Some(q));
            assert_eq!(r.cohort, rows.len());
        }
    }
    // Ten p-values by hand: sorted 0.001 0.008 0.012 0.03 0.04 0.045 0.2 0.3 0.6 0.9;
    // m/j·p = 0.01 0.04 0.04 0.075 0.08 0.075 0.2857 0.375 0.6667 0.9; running minimum from the top.
    let p = [0.03, 0.001, 0.9, 0.012, 0.2, 0.045, 0.6, 0.008, 0.04, 0.3];
    let want = [0.075, 0.01, 0.9, 0.04, 2.0 / 7.0, 0.075, 2.0 / 3.0, 0.04, 0.075, 0.375];
    for (a, b) in benjamini_hochberg(&p).iter().zip(want) {
        assert!((a - b).abs() < 1e-12);
    }
}

#[test]
fn a_strategy_links_to_its_post_and_the_feed_row_links_back() {
    let (dir, inputs) = built();
    let site = Site::new(&inputs, 0);
    for s in &site.strategies {
        let page = Html::parse_document(&read(dir.path(), &format!("strategies/{}.html", s.id())));
        let doc = s.documents.first().expect("a post");
        let want = format!("../documents/{}.html", doc.id);
        assert!(
            page.select(&sel("a[href]")).any(|a| a.value().attr("href") == Some(want.as_str())),
            "strategy {} links to its post",
            s.id()
        );
        let hyp = format!("../hypotheses/{}.html", s.submission.hypothesis_id);
        assert!(page.select(&sel("a[href]")).any(|a| a.value().attr("href") == Some(hyp.as_str())));
        let feed = Html::parse_document(&read(dir.path(), "feed.html"));
        let row = sel(&format!("tr#doc-{} a[href]", doc.id));
        let back = format!("strategies/{}.html", s.id());
        assert!(
            feed.select(&row).any(|a| a.value().attr("href") == Some(back.as_str())),
            "the feed row of {} links to {}",
            doc.id,
            s.id()
        );
        let docpage = Html::parse_document(&read(dir.path(), &format!("documents/{}.html", doc.id)));
        let back = format!("../strategies/{}.html", s.id());
        assert!(docpage.select(&sel("a[href]")).any(|a| a.value().attr("href") == Some(back.as_str())));
    }
}

#[test]
fn the_red_flags_fire_on_the_fixture_built_for_them() {
    let inputs = load();
    let site = Site::new(&inputs, 0);
    let cliff = site
        .strategies
        .iter()
        .find(|s| s.submission.program_text.contains("strategy cup_and_handle_1975"))
        .expect("the red-flag fixture");
    let flags = red_flags(cliff);
    for name in [
        "One market carries it",
        "Too few trades",
        "Profits before 1990",
        "Thin edge",
        "Collapses out of sample",
        "Parameter cliff",
        "Deep or long drawdown",
        "Fat left tail",
    ] {
        let f = flags.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("{}", name));
        assert_eq!(f.fires, Some(true), "{}: {}", name, f.value);
    }
    let clean = site
        .strategies
        .iter()
        .find(|s| s.submission.program_text.contains("strategy gold_cup_and_handle "))
        .expect("the clean fixture");
    let flags = red_flags(clean);
    for name in [
        "One market carries it",
        "Too few trades",
        "Profits before 1990",
        "Thin edge",
        "Collapses out of sample",
        "Parameter cliff",
        "Deep or long drawdown",
        "Fat left tail",
    ] {
        let f = flags.iter().find(|f| f.name == name).unwrap();
        assert_eq!(f.fires, Some(false), "{}: {}", name, f.value);
    }
    // The page marks the rows that fire.
    let dir = tempfile::tempdir().unwrap();
    build(&inputs, dir.path()).unwrap();
    let page = Html::parse_document(&read(dir.path(), &format!("strategies/{}.html", cliff.id())));
    assert_eq!(page.select(&sel("table.flags tr.fires")).count(), 9, "eight computed rows and the author's setup");
    let page = Html::parse_document(&read(dir.path(), &format!("strategies/{}.html", clean.id())));
    assert_eq!(page.select(&sel("table.flags tr.fires")).count(), 1, "the author's setup differs (no costs)");
}

#[test]
fn the_html_is_well_formed_and_every_link_resolves_inside_the_directory() {
    let (dir, _) = built();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "html") {
                out.push(p);
            }
        }
    }
    let mut pages = Vec::new();
    walk(dir.path(), &mut pages);
    assert!(pages.len() > 70);
    let root = dir.path().canonicalize().unwrap();
    let mut links = 0;
    for p in &pages {
        let text = std::fs::read_to_string(p).unwrap();
        assert!(text.starts_with("<!DOCTYPE html>"), "{}", p.display());
        let html = Html::parse_document(&text);
        assert!(html.errors.is_empty(), "{}: {:?}", p.display(), html.errors);
        assert_eq!(html.select(&sel("html[lang=en] > head > title")).count(), 1);
        assert_eq!(html.select(&sel("main")).count(), 1);
        assert_eq!(html.select(&sel("footer table.offsets")).count(), 1, "{} carries the offsets", p.display());
        assert!(html.select(&sel("footer time[datetime]")).next().is_some(), "{} carries the build time", p.display());
        assert!(html.select(&sel("script[src]")).next().is_none(), "no external script");
        let base = p.parent().unwrap();
        let links_sel = sel("a[href], link[href]");
        let hrefs: Vec<String> = html.select(&links_sel).filter_map(|a| a.value().attr("href").map(str::to_string)).collect();
        for href in hrefs {
            if href.starts_with("http://") || href.starts_with("https://") || href.starts_with('#') {
                continue;
            }
            let path = href.split('#').next().unwrap();
            let target = base.join(path).canonicalize().unwrap_or_else(|e| panic!("{}: {} -> {}: {}", p.display(), href, path, e));
            assert!(target.starts_with(&root), "{}: {} leaves the directory", p.display(), href);
            assert!(target.is_file(), "{}: {}", p.display(), href);
            links += 1;
        }
    }
    assert!(links > 500, "{} links", links);
}

#[test]
fn a_tombstoned_document_shows_its_id_and_no_text() {
    let (dir, inputs) = built();
    let t = &inputs.tombstones[0];
    let d = &inputs.documents[&t.subject_id];
    assert!(d.source_deleted && d.text.is_empty());
    let page = read(dir.path(), &format!("documents/{}.html", d.id));
    assert!(page.contains("source deleted") && page.contains(&d.id) && page.contains(d.raw_ids.as_ref().unwrap()[0].as_str()));
    assert!(!page.contains("FREE signals"));
    let feed = read(dir.path(), "feed.html");
    assert!(!feed.contains("FREE signals"));
    let ops = read(dir.path(), "operations.html");
    assert!(ops.contains(&format!("documents/{}.html", d.id)));
}

/// `cargo test -p abt-report --test site -- --ignored --nocapture projected`:
/// the fixtures' eight submissions cloned to a thousand.
#[test]
#[ignore]
fn projected_build_time_for_a_thousand_submissions() {
    use pipeline_wire::ids::Identified;
    let mut inputs = load();
    let subs: Vec<_> = inputs.submissions.values().cloned().collect();
    let outs: Vec<_> = inputs.outcomes.clone();
    let mut k = 0u64;
    while inputs.submissions.len() < 1000 {
        for s in &subs {
            k += 1;
            let mut c = s.clone();
            c.trials_used += k;
            c.r += (k as i64) * 60 * 1_000_000_000;
            let old = c.id.clone();
            c.id = c.id();
            for o in outs.iter().filter(|o| o.submission_id == old) {
                let mut o = o.clone();
                o.submission_id = c.id.clone();
                o.id = o.id();
                inputs.outcomes.push(o);
            }
            inputs.submissions.insert(c.id.clone(), c);
            if inputs.submissions.len() >= 1000 {
                break;
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let start = std::time::Instant::now();
    let b = build(&inputs, dir.path()).unwrap();
    println!(
        "{} pages for {} strategies in {:.3} s (render {:.3} s)",
        b.pages.len(),
        b.strategies,
        start.elapsed().as_secs_f64(),
        b.elapsed.as_secs_f64()
    );
}

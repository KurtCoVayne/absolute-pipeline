//! The site's views, derived once from the inputs: a strategy per
//! submission with its provenance chain and tables, the accounts, the
//! leaderboard's order (PL-P1: forward DSR, then track-record length,
//! never in-sample Sharpe) with the cohort's BH adjustment per calendar
//! week of submission (PL-6.6 item 7), and the walk from a post to every
//! strategy it caused (GP-6). `build` writes every page of PL-7.4.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use pipeline_wire::{Document, Hypothesis, Outcome, Submission, Time, Triage, Verdict};
use serde_json::Value;

use crate::inputs::Inputs;
use crate::stats::{benjamini_hochberg, iso_week};
use crate::Table;

pub struct Strategy<'a> {
    pub submission: &'a Submission,
    pub hypothesis: Option<&'a Hypothesis>,
    /// The hypothesis's documents, in its order, those the log has.
    pub documents: Vec<&'a Document>,
    /// By reveal, ascending.
    pub outcomes: Vec<&'a Outcome>,
    pub dev: Option<&'a BTreeMap<String, Table>>,
    pub forward: Option<&'a BTreeMap<String, Table>>,
    pub handle: String,
}

impl<'a> Strategy<'a> {
    pub fn id(&self) -> &str {
        &self.submission.id
    }

    pub fn table(&self, name: &str) -> Option<&'a Table> {
        self.dev?.get(name)
    }

    pub fn forward_table(&self, name: &str) -> Option<&'a Table> {
        self.forward?.get(name)
    }

    pub fn summary(&self) -> Option<&'a Table> {
        self.table("summary")
    }

    /// The summary row of `run` over `window` (`full`, `insample`, `oos`).
    pub fn summary_row(&self, run: &str, window: &str) -> Option<usize> {
        let t = self.summary()?;
        (0..t.len()).find(|&i| t.str(i, "run") == Some(run) && t.str(i, "window") == Some(window))
    }

    /// The forward plan's `base` row over the forward window.
    pub fn forward_row(&self) -> Option<(&'a Table, usize)> {
        let t = self.forward_table("summary")?;
        let i = (0..t.len()).find(|&i| t.str(i, "run") == Some("base") && t.str(i, "window") == Some("forward"))?;
        Some((t, i))
    }

    pub fn author_reported(&self) -> Option<Value> {
        let h = self.hypothesis?;
        let s = h.author_reported.as_deref()?;
        serde_json::from_str(s).ok()
    }

    pub fn insample(&self) -> Value {
        serde_json::from_str(&self.submission.insample).unwrap_or(Value::Null)
    }

    pub fn latest_outcome(&self) -> Option<&'a Outcome> {
        self.outcomes.last().copied()
    }

    pub fn outcome_metrics(o: &Outcome) -> Value {
        serde_json::from_str(&o.metrics).unwrap_or(Value::Null)
    }

    /// The forward DSR of the latest reveal.
    pub fn forward_dsr(&self) -> Option<f64> {
        Self::outcome_metrics(self.latest_outcome()?).get("dsr")?.as_f64()
    }

    /// (observed, required) of the minimum track-record length, latest reveal.
    pub fn track_record(&self) -> Option<(f64, f64)> {
        let m = Self::outcome_metrics(self.latest_outcome()?);
        let t = m.get("min_track_record")?;
        Some((t.get("observed")?.as_f64()?, t.get("required")?.as_f64()?))
    }

    /// The (claim, in-sample, forward, p) pairs of an outcome.
    pub fn pairs(o: &Outcome) -> Vec<(String, String, String, Option<f64>)> {
        Self::outcome_metrics(o)
            .get("pairs")
            .and_then(|p| p.as_array())
            .map(|a| {
                a.iter()
                    .map(|p| {
                        (
                            p.get("claim").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            p.get("insample").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            p.get("forward").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            p.get("p_value").and_then(|v| v.as_f64()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The forward p-value the cohort adjustment uses: the `survives_costs`
    /// claim's, else the first claim's with a p-value, of the latest reveal.
    pub fn forward_p(&self) -> Option<f64> {
        let pairs = Self::pairs(self.latest_outcome()?);
        pairs.iter().find(|p| p.0 == "survives_costs").and_then(|p| p.3).or_else(|| pairs.iter().find_map(|p| p.3))
    }

    pub fn insample_sharpe(&self) -> Option<f64> {
        let r = self.summary_row("base", "insample")?;
        self.summary()?.f64(r, "sharpe")
    }

    pub fn has_forward(&self) -> bool {
        !self.outcomes.is_empty()
    }

    pub fn status(&self) -> String {
        match self.latest_outcome() {
            None => "insample_only".into(),
            Some(o) => format!("{:?}", o.status).to_lowercase(),
        }
    }
}

/// One row of the leaderboard.
#[derive(Clone, Debug)]
pub struct LeaderRow {
    pub submission_id: String,
    pub handle: String,
    pub statement: String,
    pub dsr: Option<f64>,
    pub track_observed: Option<f64>,
    pub track_required: Option<f64>,
    pub forward_sharpe: Option<f64>,
    pub reveals: usize,
    pub p_value: Option<f64>,
    pub q_value: Option<f64>,
    pub week: String,
    pub cohort: usize,
    pub insample_sharpe: Option<f64>,
    pub status: String,
}

#[derive(Clone, Debug, Default)]
pub struct Account {
    pub handle: String,
    pub account_id: String,
    pub tier: Option<u8>,
    pub documents: Vec<String>,
    pub hypotheses: Vec<String>,
    pub submissions: Vec<String>,
    pub verdicts: BTreeMap<String, usize>,
    /// Forward claims (holds, fails) over the latest reveals.
    pub forward_holds: usize,
    pub forward_fails: usize,
    pub cost_usd: f64,
    /// Documents per ISO week.
    pub volume: BTreeMap<String, usize>,
}

pub struct Site<'a> {
    pub inputs: &'a Inputs,
    pub strategies: Vec<Strategy<'a>>,
    /// document id → submission ids (through the hypotheses' `doc_ids`).
    pub strategies_of_doc: BTreeMap<String, Vec<String>>,
    /// hypothesis id → submission id.
    pub strategy_of_hypothesis: BTreeMap<String, String>,
    /// document id → hypothesis ids.
    pub hypotheses_of_doc: BTreeMap<String, Vec<String>>,
    /// document id → latest triage.
    pub triage_of_doc: BTreeMap<String, &'a Triage>,
    pub accounts: BTreeMap<String, Account>,
    /// Ordered by forward evidence.
    pub leaderboard: Vec<LeaderRow>,
    /// Ordered by submission time, newest first.
    pub insample_only: Vec<LeaderRow>,
    /// plan hash → submission id.
    pub strategy_of_plan: BTreeMap<String, String>,
    pub built_at: Time,
}

impl<'a> Site<'a> {
    pub fn new(inputs: &'a Inputs, built_at: Time) -> Site<'a> {
        let mut triage_of_doc: BTreeMap<String, &Triage> = BTreeMap::new();
        for t in &inputs.triage {
            let newer = triage_of_doc.get(&t.doc_id).is_none_or(|old| t.r >= old.r);
            if newer {
                triage_of_doc.insert(t.doc_id.clone(), t);
            }
        }
        let mut hypotheses_of_doc: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for h in inputs.hypotheses.values() {
            for d in h.doc_ids.iter().flatten() {
                hypotheses_of_doc.entry(d.clone()).or_default().push(h.id.clone());
            }
        }
        let mut outcomes_of: BTreeMap<String, Vec<&Outcome>> = BTreeMap::new();
        for o in &inputs.outcomes {
            outcomes_of.entry(o.submission_id.clone()).or_default().push(o);
        }
        let mut strategies = Vec::new();
        let mut strategies_of_doc: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut strategy_of_hypothesis = BTreeMap::new();
        let mut strategy_of_plan = BTreeMap::new();
        for s in inputs.submissions.values() {
            let hypothesis = inputs.hypotheses.get(&s.hypothesis_id);
            let documents: Vec<&Document> = hypothesis.map(|h| h.doc_ids.iter().flatten().filter_map(|d| inputs.documents.get(d)).collect()).unwrap_or_default();
            let mut outcomes = outcomes_of.get(&s.id).cloned().unwrap_or_default();
            outcomes.sort_by_key(|o| (o.reveal, o.r));
            // One row per reveal: the latest record of a reveal wins.
            outcomes.dedup_by_key(|o| o.reveal);
            let handle = documents.first().map(|d| d.author.handle.clone()).unwrap_or_default();
            for d in &documents {
                strategies_of_doc.entry(d.id.clone()).or_default().push(s.id.clone());
            }
            strategy_of_hypothesis.insert(s.hypothesis_id.clone(), s.id.clone());
            strategy_of_plan.insert(s.plan_hash.clone(), s.id.clone());
            if let Some(f) = &s.forward_plan_hash {
                if !f.is_empty() {
                    strategy_of_plan.insert(f.clone(), s.id.clone());
                }
            }
            strategies.push(Strategy {
                submission: s,
                hypothesis,
                documents,
                outcomes,
                dev: inputs.reports.get(&s.plan_hash),
                forward: s.forward_plan_hash.as_deref().and_then(|f| inputs.reports.get(f)),
                handle,
            });
        }
        let (leaderboard, insample_only) = Self::rank(&strategies);
        let accounts = Self::accounts(inputs, &strategies, &triage_of_doc);
        Site {
            inputs,
            strategies,
            strategies_of_doc,
            strategy_of_hypothesis,
            hypotheses_of_doc,
            triage_of_doc,
            accounts,
            leaderboard,
            insample_only,
            strategy_of_plan,
            built_at,
        }
    }

    /// The leaderboard: forward DSR descending, then track-record length
    /// descending, then the submission time; the BH adjustment of the
    /// forward p-values within each calendar week of submission.
    fn rank(strategies: &[Strategy<'a>]) -> (Vec<LeaderRow>, Vec<LeaderRow>) {
        let mut rows: Vec<LeaderRow> = strategies
            .iter()
            .map(|s| LeaderRow {
                submission_id: s.id().to_string(),
                handle: s.handle.clone(),
                statement: s.hypothesis.map(|h| h.statement.clone()).unwrap_or_default(),
                dsr: s.forward_dsr(),
                track_observed: s.track_record().map(|t| t.0),
                track_required: s.track_record().map(|t| t.1),
                forward_sharpe: s.forward_row().and_then(|(t, i)| t.f64(i, "sharpe")),
                reveals: s.outcomes.len(),
                p_value: s.forward_p(),
                q_value: None,
                week: iso_week(s.submission.r),
                cohort: 0,
                insample_sharpe: s.insample_sharpe(),
                status: s.status(),
            })
            .collect();
        // The cohort adjustment, per week, over the rows with a forward p-value.
        let mut by_week: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, r) in rows.iter().enumerate() {
            if r.p_value.is_some() {
                by_week.entry(r.week.clone()).or_default().push(i);
            }
        }
        for members in by_week.values() {
            let p: Vec<f64> = members.iter().map(|&i| rows[i].p_value.unwrap_or(1.0)).collect();
            let q = benjamini_hochberg(&p);
            for (k, &i) in members.iter().enumerate() {
                rows[i].q_value = Some(q[k]);
                rows[i].cohort = members.len();
            }
        }
        let (mut forward, mut insample): (Vec<LeaderRow>, Vec<LeaderRow>) = rows.into_iter().partition(|r| r.reveals > 0);
        let r_of = |id: &str| strategies.iter().find(|s| s.id() == id).map(|s| s.submission.r).unwrap_or(0);
        forward.sort_by(|a, b| {
            let da = a.dsr.unwrap_or(f64::NEG_INFINITY);
            let db = b.dsr.unwrap_or(f64::NEG_INFINITY);
            db.partial_cmp(&da)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.track_observed.unwrap_or(0.0).partial_cmp(&a.track_observed.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| r_of(&b.submission_id).cmp(&r_of(&a.submission_id)))
                .then_with(|| a.submission_id.cmp(&b.submission_id))
        });
        insample.sort_by(|a, b| r_of(&b.submission_id).cmp(&r_of(&a.submission_id)).then_with(|| a.submission_id.cmp(&b.submission_id)));
        (forward, insample)
    }

    fn accounts(inputs: &'a Inputs, strategies: &[Strategy<'a>], triage_of_doc: &BTreeMap<String, &'a Triage>) -> BTreeMap<String, Account> {
        let mut accounts: BTreeMap<String, Account> = BTreeMap::new();
        for f in &inputs.follows {
            let a = accounts.entry(f.handle.clone()).or_default();
            a.handle = f.handle.clone();
            a.account_id = f.account_id.clone();
            a.tier = Some(f.tier);
        }
        let mut handle_of_doc: BTreeMap<&str, &str> = BTreeMap::new();
        for d in inputs.documents.values() {
            handle_of_doc.insert(&d.id, &d.author.handle);
            let a = accounts.entry(d.author.handle.clone()).or_default();
            if a.handle.is_empty() {
                a.handle = d.author.handle.clone();
                a.account_id = d.author.id.clone();
            }
            a.documents.push(d.id.clone());
            *a.volume.entry(iso_week(d.r)).or_default() += 1;
            if let Some(t) = triage_of_doc.get(&d.id) {
                *a.verdicts.entry(format!("{:?}", t.verdict).to_lowercase()).or_default() += 1;
            }
        }
        for t in &inputs.triage {
            if let Some(h) = handle_of_doc.get(t.doc_id.as_str()) {
                if let Some(a) = accounts.get_mut(*h) {
                    a.cost_usd += t.cost_usd;
                }
            }
        }
        for h in inputs.hypotheses.values() {
            let handle = h.doc_ids.iter().flatten().find_map(|d| handle_of_doc.get(d.as_str()).copied());
            if let Some(a) = handle.and_then(|h| accounts.get_mut(h)) {
                a.hypotheses.push(h.id.clone());
                a.cost_usd += h.cost_usd;
            }
        }
        for s in strategies {
            if let Some(a) = accounts.get_mut(&s.handle) {
                a.submissions.push(s.id().to_string());
                a.cost_usd += s.submission.cost_usd;
                if let Some(o) = s.latest_outcome() {
                    for p in Strategy::pairs(o) {
                        match p.2.as_str() {
                            "holds" => a.forward_holds += 1,
                            "fails" => a.forward_fails += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        for a in accounts.values_mut() {
            a.documents.sort_by_key(|id| std::cmp::Reverse(inputs.documents.get(id).map(|d| d.r).unwrap_or(0)));
        }
        accounts
    }

    pub fn strategy(&self, id: &str) -> Option<&Strategy<'a>> {
        self.strategies.iter().find(|s| s.id() == id)
    }

    /// The documents newest first, by receipt.
    pub fn documents_by_recency(&self) -> Vec<&'a Document> {
        let mut docs: Vec<&Document> = self.inputs.documents.values().collect();
        docs.sort_by_key(|d| std::cmp::Reverse((d.r, d.id.clone())));
        docs
    }

    /// Every instrument symbol the feed can filter by.
    pub fn instruments(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for d in self.inputs.documents.values() {
            for i in d.entities.iter().flat_map(|e| e.instruments.iter().flatten()) {
                out.insert(i.symbol.clone());
            }
        }
        for t in &self.inputs.triage {
            for i in t.instruments.iter().flatten() {
                out.insert(i.clone());
            }
        }
        out
    }

    pub fn verdict_name(v: Verdict) -> &'static str {
        match v {
            Verdict::None => "none",
            Verdict::Commentary => "commentary",
            Verdict::Claim => "claim",
            Verdict::Candidate => "candidate",
        }
    }
}

/// What a build wrote.
#[derive(Debug)]
pub struct Built {
    pub out: PathBuf,
    pub pages: Vec<PathBuf>,
    pub elapsed: std::time::Duration,
    pub strategies: usize,
}

/// Renders every page of PL-7.4 into `out`.
pub fn build(inputs: &Inputs, out: &Path) -> Result<Built> {
    let start = Instant::now();
    let site = Site::new(inputs, pipeline_wire::time::now());
    let pages = crate::render::write_all(&site, out).with_context(|| out.display().to_string())?;
    Ok(Built {
        out: out.to_path_buf(),
        pages,
        elapsed: start.elapsed(),
        strategies: site.strategies.len(),
    })
}

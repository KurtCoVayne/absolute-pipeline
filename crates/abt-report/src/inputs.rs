//! What the site is built from (contracts.md section 8): the latest version
//! of every record on the six topics, the data layer's `pipeline.*` rows,
//! the report tables of every submission's plan and forward plan, the
//! machine's facts, and where each input was read up to.

use std::collections::{BTreeMap, BTreeSet};

use pipeline_wire::{BudgetRow, Campaign, Document, Follow, Hypothesis, Job, Outcome, Record, Submission, Tombstone, Triage};
use serde_json::Value;

use crate::Table;

/// How far an input was read: a topic's partitions and their next offsets,
/// or a fixture file and its hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offset {
    pub input: String,
    pub detail: String,
    pub records: u64,
}

#[derive(Default, Debug)]
pub struct Inputs {
    /// The latest version of every document, by id; a tombstoned document
    /// is here with `source_deleted` set and its text emptied.
    pub documents: BTreeMap<String, Document>,
    /// Every triage record, in log order.
    pub triage: Vec<Triage>,
    pub hypotheses: BTreeMap<String, Hypothesis>,
    pub submissions: BTreeMap<String, Submission>,
    /// Every outcome, in log order.
    pub outcomes: Vec<Outcome>,
    /// The `control` topic's latest row per subject.
    pub follows: Vec<Follow>,
    pub budgets: Vec<BudgetRow>,
    pub tombstones: Vec<Tombstone>,
    pub campaigns: Vec<Campaign>,
    /// Jobs on the dead-letter topics, when a reader was given.
    pub dlq: Vec<Job>,
    /// `pipeline.<relation>` → rows.
    pub pipeline: BTreeMap<String, Table>,
    /// plan hash → table name → rows, for every development and forward plan.
    pub reports: BTreeMap<String, BTreeMap<String, Table>>,
    /// The machine's facts (`--facts FILE`), shown on the Operations page.
    pub facts: Option<Value>,
    pub offsets: Vec<Offset>,
}

impl Inputs {
    /// Folds one record in: the latest version per id for documents, the
    /// latest row per subject on `control`, everything else appended.
    pub fn absorb(&mut self, record: Record) {
        match record {
            Record::Document(d) => {
                let keep = match self.documents.get(&d.id) {
                    Some(old) => d.version >= old.version,
                    None => true,
                };
                if keep {
                    self.documents.insert(d.id.clone(), *d);
                }
            }
            Record::Triage(t) => self.triage.push(*t),
            Record::Hypothesis(h) => {
                self.hypotheses.insert(h.id.clone(), *h);
            }
            Record::Submission(s) => {
                self.submissions.insert(s.id.clone(), *s);
            }
            Record::Outcome(o) => self.outcomes.push(*o),
            Record::Follow(f) => {
                self.follows.retain(|x| x.handle != f.handle);
                self.follows.push(*f);
            }
            Record::BudgetRow(b) => {
                self.budgets.retain(|x| !(x.scope == b.scope && x.period == b.period));
                self.budgets.push(*b);
            }
            Record::Tombstone(t) => self.tombstones.push(*t),
            Record::Campaign(c) => {
                self.campaigns.retain(|x| x.id != c.id);
                self.campaigns.push(*c);
            }
            Record::Job(j) => self.dlq.push(*j),
            Record::RawItem(_) => {}
        }
    }

    /// Applies the tombstones (PL-5.5): a document's text goes, its id and
    /// hashes stay; an account's every document likewise.
    pub fn apply_tombstones(&mut self) {
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for t in &self.tombstones {
            match t.subject {
                pipeline_wire::TombstoneSubject::Document => {
                    gone.insert(t.subject_id.clone());
                }
                pipeline_wire::TombstoneSubject::Account => {
                    for d in self.documents.values() {
                        if d.author.id == t.subject_id {
                            gone.insert(d.id.clone());
                        }
                    }
                }
            }
        }
        for id in gone {
            if let Some(d) = self.documents.get_mut(&id) {
                d.source_deleted = true;
                d.text = String::new();
                d.segments = None;
                d.attachments = None;
            }
        }
    }

    /// The plan hashes the site needs reports for: every submission's plan
    /// and forward plan.
    pub fn plan_hashes(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for s in self.submissions.values() {
            out.insert(s.plan_hash.clone());
            if let Some(f) = &s.forward_plan_hash {
                if !f.is_empty() {
                    out.insert(f.clone());
                }
            }
        }
        out
    }

    pub fn report(&self, plan: &str, table: &str) -> Option<&Table> {
        self.reports.get(plan)?.get(table)
    }
}

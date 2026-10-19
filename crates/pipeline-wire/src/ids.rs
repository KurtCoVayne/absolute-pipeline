//! Ids (PL-4.1, PL-P4): BLAKE3 over the RFC 8785 canonical JSON of a
//! record's *logical* fields, rendered by `abt-wire`'s `Canonical`. The
//! logical fields are the table of PL-4.1 without the id itself and without
//! what the id must not depend on (a job's state, a document's later
//! versions). A `RawItem`'s id is BLAKE3 of its bytes. On the wire an id is
//! its 64 hex digits (PL-D1: a string field, so no generated table wraps
//! it; the bytes are one `from_hex` away).

use abt_wire::hash::Hash;
use abt_wire::Canonical;
use serde_json::{json, Value};

use crate::generated::pipeline::wire::{Campaign, Document, Hypothesis, Job, Outcome, RawItem, Submission, Tombstone, Triage};

/// An id: a BLAKE3 hash, written as hex.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Id(pub Hash);

impl Id {
    pub fn hex(&self) -> String {
        self.0.hex()
    }
    pub fn from_hex(s: &str) -> Result<Id, abt_wire::hash::HashParseError> {
        Hash::from_hex(s).map(Id)
    }
    pub fn of_bytes(bytes: &[u8]) -> Id {
        Id(Hash::of(bytes))
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.hex())
    }
}

/// Whether `s` is an id's rendering: 64 lowercase hex digits.
pub fn is_id(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What has logical fields and therefore an id.
pub trait Identified {
    /// The logical fields as JSON (object members in any order: the
    /// canonical form sorts them).
    fn logical(&self) -> Value;
    fn id(&self) -> String {
        id_of(&self.logical()).hex()
    }
    /// Whether the record's `id` field is the hash of its logical fields.
    fn id_holds(&self, stored: &str) -> bool {
        stored == self.id()
    }
}

/// BLAKE3 over the canonical JSON of `v`.
pub fn id_of(v: &Value) -> Id {
    let text = v.canonical().expect("JSON without NaN or infinity has a canonical form");
    Id(Hash::of(text.as_bytes()))
}

fn hex(id: &str) -> String {
    id.to_string()
}

fn hexes(ids: &Option<Vec<String>>) -> Value {
    Value::Array(ids.iter().flatten().map(|i| Value::String(i.clone())).collect())
}

fn strings(v: &Option<Vec<String>>) -> Value {
    Value::Array(v.iter().flatten().map(|s| Value::String(s.clone())).collect())
}

impl RawItem {
    /// `id = BLAKE3(bytes)` (PL-4.1): over the inline bytes, or the bytes the
    /// object store holds under `object`, which the caller passes.
    pub fn id_of_bytes(bytes: &[u8]) -> String {
        Id::of_bytes(bytes).hex()
    }
}

impl Identified for Document {
    fn logical(&self) -> Value {
        json!({
            "source": format!("{:?}", self.source),
            "author": { "id": self.author.id, "handle": self.author.handle },
            "source_id": self.parents.iter().flatten().map(|p| p.source_id.clone()).collect::<Vec<_>>(),
            "p": self.p,
            "fixed_at": self.fixed_at,
            "r": self.r,
            "raw_ids": hexes(&self.raw_ids),
        })
    }
}

impl Identified for Triage {
    fn logical(&self) -> Value {
        json!({ "doc_id": hex(&self.doc_id), "doc_version": self.doc_version, "model": self.model, "prompt_hash": hex(&self.prompt_hash), "verdict": format!("{:?}", self.verdict), "r": self.r })
    }
}

impl Identified for Hypothesis {
    fn logical(&self) -> Value {
        json!({ "doc_ids": hexes(&self.doc_ids), "h": self.h, "statement": self.statement, "universe": self.universe, "signal": self.signal, "direction": format!("{:?}", self.direction),
                "claims": strings(&self.claims), "agent": self.agent, "model": self.model, "prompt_hash": hex(&self.prompt_hash), "skills": strings(&self.skills), "r": self.r })
    }
}

impl Identified for Submission {
    fn logical(&self) -> Value {
        json!({ "hypothesis_id": hex(&self.hypothesis_id), "program_hash": hex(&self.program_hash), "plan_hash": hex(&self.plan_hash), "lineage": hex(&self.lineage), "h": self.h,
                "claims": strings(&self.claims), "trials_used": self.trials_used, "program_text": self.program_text, "plan_text": self.plan_text, "agent": self.agent, "model": self.model, "r": self.r })
    }
}

impl Identified for Outcome {
    fn logical(&self) -> Value {
        json!({ "submission_id": hex(&self.submission_id), "reveal": self.reveal, "r": self.r, "forward_from": self.forward_from, "forward_to": self.forward_to })
    }
}

impl Identified for Job {
    /// `BLAKE3(stage, inputs, definition_hash, skills_hash, model)` (PL-4.1):
    /// a redelivered message maps to the same job.
    fn logical(&self) -> Value {
        json!({ "stage": format!("{:?}", self.stage), "inputs": hexes(&self.inputs), "definition_hash": hex(&self.definition_hash), "skills_hash": hex(&self.skills_hash), "model": self.model })
    }
}

impl Identified for Tombstone {
    fn logical(&self) -> Value {
        json!({ "subject": format!("{:?}", self.subject), "subject_id": self.subject_id, "reason": self.reason, "r": self.r })
    }
}

impl Identified for Campaign {
    fn logical(&self) -> Value {
        json!({ "name": self.name, "skills_version": self.skills_version, "posts": strings(&self.posts), "r": self.r })
    }
}

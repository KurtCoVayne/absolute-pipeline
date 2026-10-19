//! A rendering for people (`pipeline wire decode FILE`, PL-5.8): the record
//! as JSON with ids as hex and times as ISO-8601, which `serde_json` over the
//! owned types does not give (ids are byte arrays, times integers).

use serde_json::{Map, Value};

use crate::generated::pipeline::wire::Record;
use crate::time::iso;

/// The fields that are times (nanoseconds) in every record.
const TIME_FIELDS: &[&str] = &["p", "r", "r_n", "fixed_at", "h", "started", "finished", "forward_from", "forward_to"];

/// The record as readable JSON.
pub fn render(record: &Record) -> Value {
    let (kind, v) = match record {
        Record::RawItem(x) => ("RawItem", serde_json::to_value(x)),
        Record::Document(x) => ("Document", serde_json::to_value(x)),
        Record::Triage(x) => ("Triage", serde_json::to_value(x)),
        Record::Hypothesis(x) => ("Hypothesis", serde_json::to_value(x)),
        Record::Submission(x) => ("Submission", serde_json::to_value(x)),
        Record::Outcome(x) => ("Outcome", serde_json::to_value(x)),
        Record::Job(x) => ("Job", serde_json::to_value(x)),
        Record::Tombstone(x) => ("Tombstone", serde_json::to_value(x)),
        Record::Follow(x) => ("Follow", serde_json::to_value(x)),
        Record::BudgetRow(x) => ("BudgetRow", serde_json::to_value(x)),
        Record::Campaign(x) => ("Campaign", serde_json::to_value(x)),
    };
    let mut v = readable(v.unwrap_or(Value::Null));
    if let Value::Object(m) = &mut v {
        m.insert("record".into(), Value::String(kind.into()));
    }
    v
}

fn readable(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                let v = if k == "bytes" {
                    match &v {
                        Value::Array(a) => Value::String(format!("<{} bytes>", a.len())),
                        other => other.clone(),
                    }
                } else if TIME_FIELDS.contains(&k.as_str()) {
                    match v.as_i64() {
                        Some(t) if t != 0 => Value::String(iso(t)),
                        _ => v,
                    }
                } else {
                    readable(v)
                };
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(readable).collect()),
        other => other,
    }
}

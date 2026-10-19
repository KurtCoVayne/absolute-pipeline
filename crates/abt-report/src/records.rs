//! The render shape (PL-D2) read back: `pipeline_wire::json::render` writes
//! times as ISO-8601 and names the table in `record`; the fixtures and the
//! `--fixtures` path hold records in that shape, so the site parses them
//! into the same owned types the broker's envelopes decode to.

use anyhow::{anyhow, bail, Context, Result};
use pipeline_wire::{parse_iso, BudgetRow, Campaign, Document, Follow, Hypothesis, Job, Outcome, Record, Submission, Tombstone, Triage};
use serde_json::{Map, Value};

/// The fields that are times in every record (as `json.rs`).
const TIME_FIELDS: &[&str] = &["p", "r", "r_n", "fixed_at", "h", "started", "finished", "forward_from", "forward_to"];

/// A record from its rendering.
pub fn from_render(v: &Value) -> Result<Record> {
    let kind = v.get("record").and_then(Value::as_str).ok_or_else(|| anyhow!("a record without a `record` member"))?.to_string();
    let v = unreadable(v.clone());
    let r = match kind.as_str() {
        "Document" => Record::Document(Box::new(serde_json::from_value::<Document>(v)?)),
        "Triage" => Record::Triage(Box::new(serde_json::from_value::<Triage>(v)?)),
        "Hypothesis" => Record::Hypothesis(Box::new(serde_json::from_value::<Hypothesis>(v)?)),
        "Submission" => Record::Submission(Box::new(serde_json::from_value::<Submission>(v)?)),
        "Outcome" => Record::Outcome(Box::new(serde_json::from_value::<Outcome>(v)?)),
        "Job" => Record::Job(Box::new(serde_json::from_value::<Job>(v)?)),
        "Tombstone" => Record::Tombstone(Box::new(serde_json::from_value::<Tombstone>(v)?)),
        "Follow" => Record::Follow(Box::new(serde_json::from_value::<Follow>(v)?)),
        "BudgetRow" => Record::BudgetRow(Box::new(serde_json::from_value::<BudgetRow>(v)?)),
        "Campaign" => Record::Campaign(Box::new(serde_json::from_value::<Campaign>(v)?)),
        "RawItem" => bail!("raw items are not an input of the site"),
        other => bail!("unknown record `{}`", other),
    };
    Ok(r)
}

/// Every record of a JSON array in the render shape.
pub fn all_from_render(v: &Value) -> Result<Vec<Record>> {
    let items = v.as_array().ok_or_else(|| anyhow!("the file is not a JSON array of records"))?;
    items.iter().enumerate().map(|(i, r)| from_render(r).with_context(|| format!("record {}", i))).collect()
}

/// The inverse of `json::readable`: ISO times back to nanoseconds, the
/// `record` member dropped.
fn unreadable(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                if k == "record" {
                    continue;
                }
                let v = if TIME_FIELDS.contains(&k.as_str()) {
                    match &v {
                        Value::String(s) => parse_iso(s).map(Value::from).unwrap_or(v),
                        _ => v,
                    }
                } else {
                    unreadable(v)
                };
                out.insert(k, v);
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(unreadable).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pipeline_wire::{json, Author, Source};

    #[test]
    fn a_rendered_document_reads_back() {
        let d = Document {
            id: "00".repeat(32),
            version: 1,
            raw_ids: None,
            source: Source::X,
            author: Box::new(Author { id: "1".into(), handle: "a".into() }),
            p: 1_758_000_000_000_000_000,
            fixed_at: 1_758_000_000_000_000_000,
            r: 1_758_000_000_000_000_001,
            r_n: 1_758_000_000_000_000_001,
            parents: None,
            text: "t".into(),
            segments: None,
            entities: None,
            links: None,
            attachments: None,
            lang: Some("en".into()),
            flags: None,
            enrichment: None,
            source_deleted: false,
        };
        let rec = Record::Document(Box::new(d.clone()));
        let back = from_render(&json::render(&rec)).unwrap();
        assert_eq!(back, rec);
    }
}

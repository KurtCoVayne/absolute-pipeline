//! `pipeline.*` in the data layer (PL-7.2, contracts section 6): the
//! relations the gateway alone writes, through `put` under the class
//! `pipeline`, with the columns of PL-7.2 (ids as labels, hex). Beyond
//! PL-7.2's columns a submission carries the texts the forward scheduler
//! reveals from (`program_text`, `plan_text`, `forward_plan_text`) and the
//! forward plan's hash, and an outcome its verdicts and metrics as JSON
//! (PL-D104: the data layer is the single store, A-D20, so a reveal needs
//! no broker to find its plan). The report site reads them with `scan`.

use std::sync::Arc;

use abt_wire::generated::abt::wire::{ColumnSpec, ColumnType, QueryForm, RelationSchema, Role, Scan};
use arrow::array::{ArrayRef, BinaryArray, Float64Array, Int64Array, RecordBatch, StringArray, TimestampNanosecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use serde_json::Value;

use crate::layers::{LayerError, Layers};
use crate::table::Table;

pub const DOCUMENT: &str = "pipeline.document";
pub const HYPOTHESIS: &str = "pipeline.hypothesis";
pub const SUBMISSION: &str = "pipeline.submission";
pub const OUTCOME: &str = "pipeline.outcome";
pub const AUDIT: &str = "pipeline.audit";

/// A column's values.
pub enum Col {
    Time(Vec<Option<i64>>),
    Num(Vec<Option<f64>>),
    Count(Vec<Option<i64>>),
    Label(Vec<Option<String>>),
    Bytes(Vec<Option<String>>),
}

impl Col {
    fn ty(&self) -> ColumnType {
        match self {
            Col::Time(_) => ColumnType::Time,
            Col::Num(_) => ColumnType::Num,
            Col::Count(_) => ColumnType::Count,
            Col::Label(_) => ColumnType::Label,
            Col::Bytes(_) => ColumnType::Bytes,
        }
    }

    fn array(&self) -> (DataType, ArrayRef) {
        match self {
            Col::Time(v) => (DataType::Timestamp(TimeUnit::Nanosecond, None), Arc::new(TimestampNanosecondArray::from(v.clone()))),
            Col::Num(v) => (DataType::Float64, Arc::new(Float64Array::from(v.clone()))),
            Col::Count(v) => (DataType::Int64, Arc::new(Int64Array::from(v.clone()))),
            Col::Label(v) => (DataType::Utf8, Arc::new(StringArray::from(v.clone()))),
            Col::Bytes(v) => (DataType::Binary, Arc::new(BinaryArray::from(v.iter().map(|s| s.as_deref().map(str::as_bytes)).collect::<Vec<_>>()))),
        }
    }
}

/// A relation's rows being built: its columns with their roles (D-2.2).
pub struct Rows {
    name: String,
    cols: Vec<(String, Role, Col)>,
}

impl Rows {
    pub fn new(name: &str) -> Rows {
        Rows { name: name.to_string(), cols: vec![] }
    }

    pub fn col(mut self, name: &str, role: Role, c: Col) -> Rows {
        self.cols.push((name.to_string(), role, c));
        self
    }

    pub fn finish(self) -> Result<(RelationSchema, RecordBatch), String> {
        let schema = RelationSchema {
            name: self.name.clone(),
            class: "pipeline".into(),
            columns: self
                .cols
                .iter()
                .map(|(n, role, c)| ColumnSpec {
                    name: n.clone(),
                    ty: c.ty(),
                    role: *role,
                    dimension: None,
                })
                .collect(),
            calendar: None,
            constraints: None,
            attributes: None,
            schema_version: Some("1.0".into()),
        };
        let (fields, arrays): (Vec<Field>, Vec<ArrayRef>) = self
            .cols
            .iter()
            .map(|(n, _, c)| {
                let (ty, a) = c.array();
                (Field::new(n, ty, true), a)
            })
            .unzip();
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays).map_err(|e| format!("{}: {}", self.name, e))?;
        Ok((schema, batch))
    }
}

fn one(s: &str) -> Col {
    Col::Label(vec![Some(s.to_string())])
}

fn text(s: &str) -> Col {
    Col::Bytes(vec![Some(s.to_string())])
}

fn time(t: i64) -> Col {
    Col::Time(vec![Some(t)])
}

fn count(n: i64) -> Col {
    Col::Count(vec![Some(n)])
}

/// A document's row (PL-7.2: no text; `size` is the text's length), from
/// its rendering (PL-D2).
pub fn document_row(d: &Value) -> Result<(RelationSchema, RecordBatch), String> {
    let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let t = |k: &str| d.get(k).map(time_of).unwrap_or(0);
    let author = d
        .get("author")
        .map(|a| a.get("handle").and_then(Value::as_str).or_else(|| a.get("id").and_then(Value::as_str)).unwrap_or(""))
        .unwrap_or("")
        .to_string();
    if s("id").is_empty() {
        return Err("a document has an id".into());
    }
    Rows::new(DOCUMENT)
        .col("doc_id", Role::Label, one(&s("id")))
        .col("source", Role::Label, one(&s("source")))
        .col("author", Role::Label, one(&author))
        .col("p", Role::Value, time(t("p")))
        .col("r", Role::EventTime, time(t("r")))
        .col("size", Role::Value, count(d.get("text").and_then(Value::as_str).map_or(0, |x| x.len() as i64)))
        .finish()
}

/// A hypothesis's row, from its rendering.
pub fn hypothesis_row(h: &Value) -> Result<(RelationSchema, RecordBatch), String> {
    let s = |k: &str| h.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    if s("id").is_empty() {
        return Err("a hypothesis has an id".into());
    }
    let doc_ids = h.get("doc_ids").cloned().unwrap_or(Value::Array(vec![]));
    Rows::new(HYPOTHESIS)
        .col("id", Role::Label, one(&s("id")))
        .col("doc_ids", Role::Value, text(&doc_ids.to_string()))
        .col("h", Role::Value, time(h.get("h").map(time_of).unwrap_or(0)))
        .col("agent", Role::Label, one(&s("agent")))
        .col("r", Role::EventTime, time(h.get("r").map(time_of).unwrap_or(0)))
        .finish()
}

/// A time from a rendering: ISO-8601 or nanoseconds.
pub fn time_of(v: &Value) -> i64 {
    match v {
        Value::String(s) => pipeline_wire::parse_iso(s).ok().or_else(|| abt_wire::scalar::parse_timestamp(s)).unwrap_or(0),
        Value::Number(n) => n.as_i64().unwrap_or(0),
        _ => 0,
    }
}

/// A submission's row (PL-7.2 and PL-D104's texts).
#[allow(clippy::too_many_arguments)]
pub fn submission_row(s: &pipeline_wire::Submission) -> Result<(RelationSchema, RecordBatch), String> {
    Rows::new(SUBMISSION)
        .col("id", Role::Label, one(&s.id))
        .col("hypothesis_id", Role::Label, one(&s.hypothesis_id))
        .col("program_hash", Role::Label, one(&s.program_hash))
        .col("plan_hash", Role::Label, one(&s.plan_hash))
        .col("lineage", Role::Label, one(&s.lineage))
        .col("h", Role::Value, time(s.h))
        .col("r", Role::EventTime, time(s.r))
        .col("forward_plan_hash", Role::Value, text(s.forward_plan_hash.as_deref().unwrap_or("")))
        .col("agent", Role::Value, text(&s.agent))
        .col("job", Role::Value, text(s.job_id.as_deref().unwrap_or("")))
        .col("trials_used", Role::Value, count(s.trials_used as i64))
        .col("claims", Role::Value, text(&serde_json::to_string(&s.claims.clone().unwrap_or_default()).unwrap_or_default()))
        .col("insample", Role::Value, text(&s.insample))
        .col("program_text", Role::Value, text(&s.program_text))
        .col("plan_text", Role::Value, text(&s.plan_text))
        .col("forward_plan_text", Role::Value, text(s.forward_plan_text.as_deref().unwrap_or("")))
        .finish()
}

/// An outcome's row.
pub fn outcome_row(o: &pipeline_wire::Outcome) -> Result<(RelationSchema, RecordBatch), String> {
    Rows::new(OUTCOME)
        .col("id", Role::Label, one(&o.id))
        .col("submission_id", Role::Label, one(&o.submission_id))
        .col("reveal", Role::Value, count(o.reveal as i64))
        .col("r", Role::EventTime, time(o.r))
        .col("forward_from", Role::Value, time(o.forward_from))
        .col("forward_to", Role::Value, time(o.forward_to))
        .col("status", Role::Label, one(status_name(o.status)))
        .col("verdicts", Role::Value, text(&serde_json::to_string(&o.verdicts.clone().unwrap_or_default()).unwrap_or_default()))
        .col("metrics", Role::Value, text(&o.metrics))
        .col("retired_reason", Role::Value, text(o.retired_reason.as_deref().unwrap_or("")))
        .finish()
}

pub fn status_name(s: pipeline_wire::OutcomeStatus) -> &'static str {
    match s {
        pipeline_wire::OutcomeStatus::InsampleOnly => "insample_only",
        pipeline_wire::OutcomeStatus::Forward => "forward",
        pipeline_wire::OutcomeStatus::Retired => "retired",
    }
}

/// One audit row (PL-7.1): job, request kind, relation or plan, bytes,
/// duration, outcome.
#[derive(Clone, Debug)]
pub struct AuditRow {
    /// The request's number in this gateway's life: with `r`, what makes
    /// the row its own.
    pub request: u64,
    pub job: String,
    pub kind: String,
    pub subject: String,
    pub bytes: u64,
    pub duration_ns: i64,
    pub outcome: String,
    pub r: i64,
}

pub fn audit_rows(rows: &[AuditRow]) -> Result<(RelationSchema, RecordBatch), String> {
    Rows::new(AUDIT)
        .col("request", Role::Label, Col::Label(rows.iter().map(|a| Some(a.request.to_string())).collect()))
        .col("job", Role::Label, Col::Label(rows.iter().map(|a| Some(a.job.clone())).collect()))
        .col("kind", Role::Label, Col::Label(rows.iter().map(|a| Some(a.kind.clone())).collect()))
        .col("subject", Role::Label, Col::Label(rows.iter().map(|a| Some(a.subject.clone())).collect()))
        .col("bytes", Role::Value, Col::Count(rows.iter().map(|a| Some(a.bytes as i64)).collect()))
        .col("duration_ns", Role::Value, Col::Count(rows.iter().map(|a| Some(a.duration_ns)).collect()))
        .col("outcome", Role::Label, Col::Label(rows.iter().map(|a| Some(a.outcome.clone())).collect()))
        .col("r", Role::EventTime, Col::Time(rows.iter().map(|a| Some(a.r)).collect()))
        .finish()
}

/// Every row of a relation's current version, unclamped (the gateway's
/// own reads); an empty table when the store has no such relation.
pub fn scan_all(layers: &Layers, relation: &str) -> Result<Table, LayerError> {
    let req = crate::query::request(QueryForm::Scan(Box::new(Scan {
        relation: relation.to_string(),
        t0: i64::MIN,
        t1: i64::MAX,
        predicate: None,
        columns: None,
    })));
    match layers.query(req) {
        Ok((_, batches)) => Ok(Table::of(&batches, usize::MAX)),
        Err(e) if e.is_not_found() => Ok(Table::default()),
        Err(e) => Err(e),
    }
}

/// A `put` of one relation's rows; `seq_to` written.
pub fn put(layers: &Layers, built: (RelationSchema, RecordBatch)) -> Result<u64, LayerError> {
    let (schema, batch) = built;
    layers.put(schema, &[batch]).map(|p| p.seq_to)
}

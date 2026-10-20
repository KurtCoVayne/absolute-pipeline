//! The forward scheduler (PL-7.3, PL-6.6): for every submission whose
//! latest outcome is not `retired`, when the data layer's current versions
//! extend past the last reveal by at least one report period, the stored
//! forward plan's `plan_reveal` over `(H, now]` through the query layer,
//! as the gateway itself (never an agent); the `Outcome` (the verdict
//! pairs, the metrics with the lineage's count, the forward DSR as the
//! query layer computed it) written to `pipeline.outcome` and produced to
//! the `outcomes` topic, or written as JSON to `--out DIR` when no broker
//! is configured. A submission is retired by the owner's command, by a
//! `block` claim failing on two consecutive reveals, or by a tombstone of
//! its source on `control`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use pipeline_wire::{Identified, Outcome, OutcomeStatus, Record};
use serde_json::{json, Value};

use crate::api::{data_extent, runs_of, text_for_layer};
use crate::layers::Layers;
use crate::plan::{self, Kind};
use crate::provenance::{self, scan_all, OUTCOME, SUBMISSION};

/// How the scheduler runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// The report period: a reveal is due when the data extends past the
    /// last one by at least this (nanoseconds).
    pub period: i64,
    /// Where outcomes go as JSON when there is no broker.
    pub out: Option<PathBuf>,
    /// `PIPELINE_BROKERS`, when set.
    pub brokers: Option<String>,
}

/// One tick's reveal of one submission.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Revealed {
    pub submission_id: String,
    pub outcome_id: String,
    pub reveal: u32,
    pub forward_from: i64,
    pub forward_to: i64,
    pub status: String,
    pub retired_reason: Option<String>,
}

/// A submission as `pipeline.submission` holds it.
#[derive(Clone, Debug)]
pub struct Stored {
    pub id: String,
    pub hypothesis_id: String,
    pub program_hash: String,
    pub forward_plan_hash: String,
    pub forward_plan_text: String,
    pub program_text: String,
    pub h: i64,
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

/// Every submission, by id.
pub fn submissions(layers: &Layers) -> Result<Vec<Stored>, String> {
    let t = scan_all(layers, SUBMISSION).map_err(|e| e.to_string())?;
    let mut seen = std::collections::BTreeSet::new();
    Ok(t.records()
        .into_iter()
        .filter_map(|r| {
            let id = text(r.get("id"));
            if id.is_empty() || !seen.insert(id.clone()) {
                return None;
            }
            Some(Stored {
                id,
                hypothesis_id: text(r.get("hypothesis_id")),
                program_hash: text(r.get("program_hash")),
                forward_plan_hash: text(r.get("forward_plan_hash")),
                forward_plan_text: text(r.get("forward_plan_text")),
                program_text: text(r.get("program_text")),
                h: r.get("h").map(provenance::time_of).unwrap_or(0),
            })
        })
        .collect())
}

/// An outcome as stored: the latest per submission is what the scheduler
/// reads.
#[derive(Clone, Debug)]
pub struct StoredOutcome {
    pub reveal: u32,
    pub r: i64,
    pub forward_to: i64,
    pub status: String,
    pub verdicts: Vec<String>,
}

/// The latest outcome of every submission.
pub fn latest_outcomes(layers: &Layers) -> Result<BTreeMap<String, StoredOutcome>, String> {
    let t = scan_all(layers, OUTCOME).map_err(|e| e.to_string())?;
    let mut out: BTreeMap<String, StoredOutcome> = BTreeMap::new();
    for r in t.records() {
        let sid = text(r.get("submission_id"));
        let o = StoredOutcome {
            reveal: r.get("reveal").and_then(Value::as_u64).unwrap_or(0) as u32,
            r: r.get("r").map(provenance::time_of).unwrap_or(0),
            forward_to: r.get("forward_to").map(provenance::time_of).unwrap_or(0),
            status: text(r.get("status")),
            verdicts: r.get("verdicts").and_then(Value::as_str).and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default(),
        };
        match out.get(&sid) {
            Some(cur) if (cur.reveal, cur.r) >= (o.reveal, o.r) => {}
            _ => {
                out.insert(sid, o);
            }
        }
    }
    Ok(out)
}

/// The document ids tombstoned on `control` (PL-5.5): from the broker when
/// one is configured, else from `<out>/control.jsonl` (one rendered
/// `Tombstone` per line), what a test writes.
pub fn tombstoned(cfg: &Config) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(out) = &cfg.out {
        if let Ok(text) = std::fs::read_to_string(out.join("control.jsonl")) {
            for line in text.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    if v.get("record").and_then(Value::as_str) == Some("Tombstone") {
                        for k in ["doc_id", "subject_id"] {
                            if let Some(id) = v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                                ids.push(id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    #[cfg(feature = "kafka")]
    if let Some(brokers) = &cfg.brokers {
        ids.extend(kafka::tombstones(brokers));
    }
    ids
}

/// The document ids a submission's hypothesis was made from.
fn documents_of(layers: &Layers, hypothesis_id: &str) -> Vec<String> {
    scan_all(layers, provenance::HYPOTHESIS)
        .ok()
        .map(|t| {
            t.records()
                .into_iter()
                .filter(|r| text(r.get("id")) == hypothesis_id)
                .flat_map(|r| r.get("doc_ids").and_then(Value::as_str).and_then(|j| serde_json::from_str::<Vec<String>>(j).ok()).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default()
}

/// Writes an outcome to `pipeline.outcome` and produces it (the topic, or
/// the JSON file).
fn record(layers: &Layers, cfg: &Config, o: &Outcome) -> Result<(), String> {
    provenance::put(layers, provenance::outcome_row(o)?).map_err(|e| e.to_string())?;
    let rendered = pipeline_wire::json::render(&Record::Outcome(Box::new(o.clone())));
    match &cfg.brokers {
        Some(brokers) => produce(brokers, o),
        None => {
            if let Some(out) = &cfg.out {
                std::fs::create_dir_all(out).map_err(|e| format!("{}: {}", out.display(), e))?;
                let path = out.join(format!("{}.json", o.id));
                std::fs::write(&path, serde_json::to_string_pretty(&rendered).unwrap_or_default()).map_err(|e| format!("{}: {}", path.display(), e))?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "kafka")]
fn produce(brokers: &str, o: &Outcome) -> Result<(), String> {
    kafka::produce(brokers, o)
}

#[cfg(not(feature = "kafka"))]
fn produce(brokers: &str, _o: &Outcome) -> Result<(), String> {
    Err(format!("PIPELINE_BROKERS={} but this build has no Kafka client (built without the `kafka` feature)", brokers))
}

/// A retirement row: the owner's command, a failed `block` claim twice, a
/// tombstone.
fn retire_with(layers: &Layers, cfg: &Config, s: &Stored, last: Option<&StoredOutcome>, reason: &str, now: i64) -> Result<Revealed, String> {
    let mut o = Outcome {
        id: String::new(),
        submission_id: s.id.clone(),
        reveal: last.map_or(0, |l| l.reveal),
        r: now,
        forward_from: s.h,
        forward_to: last.map_or(s.h, |l| l.forward_to),
        verdicts: Some(last.map(|l| l.verdicts.clone()).unwrap_or_default()),
        metrics: json!({ "retired": reason }).to_string(),
        status: OutcomeStatus::Retired,
        retired_reason: Some(reason.to_string()),
    };
    o.id = o.id();
    record(layers, cfg, &o)?;
    tracing::info!(submission = %s.id, reason, "submission retired");
    Ok(Revealed {
        submission_id: s.id.clone(),
        outcome_id: o.id,
        reveal: o.reveal,
        forward_from: o.forward_from,
        forward_to: o.forward_to,
        status: "retired".into(),
        retired_reason: Some(reason.to_string()),
    })
}

/// `abt-gateway retire ID --reason`: the owner's retirement.
pub fn retire(layers: &Layers, cfg: &Config, submission_id: &str, reason: &str, now: i64) -> Result<Revealed, String> {
    let subs = submissions(layers)?;
    let s = subs
        .iter()
        .find(|s| s.id == submission_id || s.id.starts_with(submission_id))
        .ok_or_else(|| format!("no submission {}", submission_id))?;
    let outcomes = latest_outcomes(layers)?;
    let last = outcomes.get(&s.id);
    if last.is_some_and(|l| l.status == "retired") {
        return Err(format!("submission {} is already retired", s.id));
    }
    retire_with(layers, cfg, s, last, reason, now)
}

/// One pass over every submission (`--once`; the loop calls it every
/// `--every`): the reveals made and the retirements written.
pub fn tick(layers: &Layers, cfg: &Config, now: i64) -> Result<Vec<Revealed>, String> {
    let subs = submissions(layers)?;
    let outcomes = latest_outcomes(layers)?;
    let extent = data_extent(layers).map_err(|e| e.to_string())?;
    let tombstones = tombstoned(cfg);
    let after = Mutex::new(None);
    let mut out = Vec::new();
    for s in &subs {
        let last = outcomes.get(&s.id);
        if last.is_some_and(|l| l.status == "retired") {
            continue;
        }
        if !tombstones.is_empty() {
            let docs = documents_of(layers, &s.hypothesis_id);
            if let Some(d) = docs.iter().find(|d| tombstones.contains(d)) {
                out.push(retire_with(layers, cfg, s, last, &format!("tombstone of document {}", d), now)?);
                continue;
            }
        }
        let last_to = last.map_or(s.h, |l| l.forward_to.max(s.h));
        let Some(extent) = extent else { continue };
        if extent - last_to < cfg.period {
            tracing::debug!(submission = %s.id, extent = %pipeline_wire::iso(extent), last = %pipeline_wire::iso(last_to), "no new period to reveal");
            continue;
        }
        match reveal_one(layers, cfg, s, last, now, &after) {
            Ok(r) => out.push(r),
            Err(e) => tracing::warn!(submission = %s.id, error = %e, "the reveal failed"),
        }
    }
    Ok(out)
}

/// A submission's next reveal: the forward plan run once in sample (when
/// it has not), then its reveal step over the whole sample, the outcome
/// written.
fn reveal_one(layers: &Layers, cfg: &Config, s: &Stored, last: Option<&StoredOutcome>, now: i64, after: &Mutex<Option<bool>>) -> Result<Revealed, String> {
    let name = plan::strategy_name(&s.program_text).ok_or("the submitted program declares no strategy")?;
    let study = plan::parse(&s.forward_plan_text)?;
    let step = study.reveal_steps().into_iter().next().ok_or("the forward plan has no reveal step")?;
    let sources = layers.sources_for(&s.program_text, &name);
    let plan_text = text_for_layer(layers, &s.forward_plan_text, &sources, &name, Kind::Forward, s.h, after).map_err(|e| e.to_string())?;
    let checked = layers.plan(&["check"], &plan_text, sources.clone(), &name).map_err(|e| e.to_string())?;
    let hash = checked["hash"].as_str().unwrap_or("").to_string();
    // The in-sample run of the forward plan, once: a reveal needs the
    // committed point the run step made.
    let ran = layers.plan(&["report"], &plan_text, sources.clone(), &name).is_ok();
    if !ran {
        layers.plan(&["run"], &plan_text, sources.clone(), &name).map_err(|e| format!("the forward plan's run: {}", e))?;
    }
    let ex = layers.plan(&["reveal", "plan", &step], &plan_text, sources.clone(), &name).map_err(|e| format!("the reveal: {}", e))?;
    let runs = runs_of(&ex);
    let revealed = ex["outcomes"].as_array().into_iter().flatten().find(|o| o["kind"] == "reveal").cloned().unwrap_or(Value::Null);
    let window = revealed["window"]
        .as_array()
        .map(|w| (w.first().and_then(Value::as_i64).unwrap_or(s.h), w.get(1).and_then(Value::as_i64).unwrap_or(s.h)));
    let (from, to) = window.unwrap_or((s.h, s.h));
    // The verdict pairs: the plan's in-sample verdicts with the reveal's.
    let stored = scan_all(layers, &format!("query.{}.verdicts", hash)).map_err(|e| e.to_string())?;
    let in_sample: BTreeMap<String, String> = stored
        .records()
        .into_iter()
        .filter(|r| r.get("in_sample") == Some(&Value::Bool(true)))
        .map(|r| (text(r.get("name")), text(r.get("verdict"))))
        .collect();
    let mut verdicts = Vec::new();
    let mut block_failed = Vec::new();
    for c in ex["claims"].as_array().into_iter().flatten().filter(|c| c["kind"] == "claim" && c["in_sample"] == false) {
        let name = c["name"].as_str().unwrap_or("").to_string();
        let out = verdict_text(&c["verdict"]);
        let before = in_sample.get(&name).cloned().unwrap_or_else(|| "unjudged".into());
        verdicts.push(format!("{}: in_sample={} forward={}", name, before, out));
        if c["policy"] == "block" && out == "fails" {
            block_failed.push(name);
        }
    }
    let mut status = OutcomeStatus::Forward;
    let mut retired_reason = None;
    if let Some(l) = last {
        for b in &block_failed {
            if l.verdicts.iter().any(|v| v.starts_with(&format!("{}: ", b)) && v.ends_with("forward=fails")) {
                status = OutcomeStatus::Retired;
                retired_reason = Some(format!("claim `{}` (block) failed on two consecutive reveals", b));
            }
        }
    }
    let metrics = json!({
        "forward_window": [pipeline_wire::iso(from), pipeline_wire::iso(to)],
        "returns": revealed["returns"],
        "trading": revealed["trading"],
        "objective": revealed["objective"],
        "trials": ex["trials"],
        "trial_kinds": ex["trial_kinds"],
        "dsr": ex["dsr"],
        "warnings": ex["warnings"],
        "runs": runs,
        "plan_hash": hash,
        "program_hash": s.program_hash,
        "lineage": ex["lineage"],
    });
    let mut o = Outcome {
        id: String::new(),
        submission_id: s.id.clone(),
        reveal: last.map_or(0, |l| l.reveal) + 1,
        r: now,
        forward_from: from,
        forward_to: to,
        verdicts: Some(verdicts),
        metrics: metrics.to_string(),
        status,
        retired_reason: retired_reason.clone(),
    };
    o.id = o.id();
    record(layers, cfg, &o)?;
    tracing::info!(submission = %s.id, reveal = o.reveal, from = %pipeline_wire::iso(from), to = %pipeline_wire::iso(to), status = provenance::status_name(status), "revealed");
    Ok(Revealed {
        submission_id: s.id.clone(),
        outcome_id: o.id,
        reveal: o.reveal,
        forward_from: from,
        forward_to: to,
        status: provenance::status_name(status).into(),
        retired_reason,
    })
}

fn verdict_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(m) => m.iter().next().map(|(k, v)| format!("{}({})", k, v.as_str().unwrap_or(""))).unwrap_or_default(),
        Value::Null => "undecidable(no verdict)".into(),
        other => other.to_string(),
    }
}

/// A duration as the command line writes it: `10s`, `30m`, `2h`, `7d`,
/// `1w`, `week`, `day`; nanoseconds.
pub fn parse_duration(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let ns = pipeline_wire::NS_PER_S;
    let named = match s {
        "week" | "weekly" => Some(7 * 86_400 * ns),
        "day" | "daily" => Some(86_400 * ns),
        "hour" | "hourly" => Some(3_600 * ns),
        "month" | "monthly" => Some(30 * 86_400 * ns),
        _ => None,
    };
    if let Some(n) = named {
        return Ok(n);
    }
    let i = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(i);
    let x: f64 = num.parse().map_err(|_| format!("`{}` is not a duration (10s, 30m, 2h, 7d, 1w)", s))?;
    let unit_ns = match unit.trim() {
        "ns" => 1.0,
        "ms" => 1e6,
        "s" | "" => ns as f64,
        "m" | "min" => 60.0 * ns as f64,
        "h" => 3_600.0 * ns as f64,
        "d" => 86_400.0 * ns as f64,
        "w" => 7.0 * 86_400.0 * ns as f64,
        _ => return Err(format!("`{}` is not a duration (10s, 30m, 2h, 7d, 1w)", s)),
    };
    Ok((x * unit_ns) as i64)
}

#[cfg(feature = "kafka")]
pub mod kafka {
    //! The `outcomes` topic through `rdkafka` (PL-4.3): the idempotent
    //! producer, `acks=all`, the record's envelope as the message with the
    //! headers of contracts section 2; the `control` topic read from its
    //! start for tombstones.

    use pipeline_wire::{Outcome, Record};
    use rdkafka::config::ClientConfig;
    use rdkafka::consumer::{BaseConsumer, Consumer};
    use rdkafka::message::{Header, Message, OwnedHeaders};
    use rdkafka::producer::{BaseProducer, BaseRecord, Producer};
    use std::time::Duration;

    pub fn produce(brokers: &str, o: &Outcome) -> Result<(), String> {
        let producer: BaseProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("enable.idempotence", "true")
            .set("acks", "all")
            .set("message.timeout.ms", "30000")
            .create()
            .map_err(|e| e.to_string())?;
        let bytes = pipeline_wire::encode(&Record::Outcome(Box::new(o.clone())));
        let headers = OwnedHeaders::new()
            .insert(Header {
                key: "schema",
                value: Some(pipeline_wire::SCHEMA),
            })
            .insert(Header {
                key: "record",
                value: Some("Outcome"),
            })
            .insert(Header {
                key: "id",
                value: Some(o.id.as_str()),
            });
        producer
            .send(BaseRecord::to(pipeline_wire::topics::OUTCOMES).key(o.submission_id.as_str()).payload(&bytes).headers(headers))
            .map_err(|(e, _)| e.to_string())?;
        producer.flush(Duration::from_secs(30)).map_err(|e| e.to_string())
    }

    /// The document and account ids of every tombstone on `control`.
    pub fn tombstones(brokers: &str) -> Vec<String> {
        let Ok(consumer) = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("group.id", "abt-gateway-forward")
            .set("auto.offset.reset", "earliest")
            .set("enable.auto.commit", "false")
            .create::<BaseConsumer>()
        else {
            return vec![];
        };
        let mut tpl = rdkafka::TopicPartitionList::new();
        tpl.add_partition_offset(pipeline_wire::topics::CONTROL, 0, rdkafka::Offset::Beginning).ok();
        if consumer.assign(&tpl).is_err() {
            return vec![];
        }
        let mut ids = Vec::new();
        let mut idle = 0;
        while idle < 5 {
            match consumer.poll(Duration::from_millis(500)) {
                Some(Ok(m)) => {
                    idle = 0;
                    if let Some(Ok(Record::Tombstone(t))) = m.payload().map(pipeline_wire::decode) {
                        ids.push(t.subject_id.clone());
                        if let Some(d) = t.doc_id.clone() {
                            ids.push(d);
                        }
                    }
                }
                _ => idle += 1,
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("10s").unwrap(), 10 * pipeline_wire::NS_PER_S);
        assert_eq!(parse_duration("week").unwrap(), 7 * 86_400 * pipeline_wire::NS_PER_S);
        assert_eq!(parse_duration("1w").unwrap(), parse_duration("7d").unwrap());
        assert_eq!(parse_duration("1.5h").unwrap(), 5_400 * pipeline_wire::NS_PER_S);
        assert!(parse_duration("soon").is_err());
    }
}

//! The `--fixtures DIR` input: the log's topics as JSON arrays in the render
//! shape (`docs.json`, `triage.json`, `hypotheses.json`, `submissions.json`,
//! `outcomes.json`, `control.json`, and `dlq.json` when present), the data
//! layer's rows as `pipeline/<relation>.json` tables, and the report tables
//! as `report/<plan hash>/<table>.json`. The whole site builds from a
//! directory, with no broker and no data service.

use std::path::Path;

use anyhow::{Context, Result};

use crate::inputs::{Inputs, Offset};
use crate::records::all_from_render;
use crate::Table;

pub const TOPIC_FILES: &[(&str, &str)] = &[
    ("docs", "docs.json"),
    ("triage", "triage.json"),
    ("hypotheses", "hypotheses.json"),
    ("submissions", "submissions.json"),
    ("outcomes", "outcomes.json"),
    ("control", "control.json"),
    ("dlq", "dlq.json"),
];

pub fn load(dir: &Path) -> Result<Inputs> {
    let mut inputs = Inputs::default();
    for (topic, file) in TOPIC_FILES {
        let path = dir.join(file);
        if !path.exists() {
            if *topic == "dlq" {
                continue;
            }
            anyhow::bail!("{}: missing", path.display());
        }
        let bytes = std::fs::read(&path).with_context(|| path.display().to_string())?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).with_context(|| path.display().to_string())?;
        let records = all_from_render(&v).with_context(|| path.display().to_string())?;
        inputs.offsets.push(Offset {
            input: topic.to_string(),
            detail: format!("{} blake3 {}", file, &blake3::hash(&bytes).to_hex()[..16]),
            records: records.len() as u64,
        });
        for r in records {
            inputs.absorb(r);
        }
    }
    inputs.apply_tombstones();
    let pipeline = dir.join("pipeline");
    if pipeline.is_dir() {
        for entry in std::fs::read_dir(&pipeline)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            let t = read_table(&path)?;
            inputs.offsets.push(Offset {
                input: format!("pipeline.{}", name),
                detail: format!("pipeline/{}.json", name),
                records: t.len() as u64,
            });
            inputs.pipeline.insert(format!("pipeline.{}", name), t);
        }
    }
    let report = dir.join("report");
    let mut tables = 0u64;
    for plan in inputs.plan_hashes() {
        let d = report.join(&plan);
        if !d.is_dir() {
            continue;
        }
        let mut by_name = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            by_name.insert(name, read_table(&path)?);
            tables += 1;
        }
        inputs.reports.insert(plan, by_name);
    }
    inputs.offsets.push(Offset {
        input: "query.<plan>.report.*".into(),
        detail: format!("report/ ({} plans)", inputs.reports.len()),
        records: tables,
    });
    Ok(inputs)
}

fn read_table(path: &Path) -> Result<Table> {
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    serde_json::from_slice(&bytes).with_context(|| path.display().to_string())
}

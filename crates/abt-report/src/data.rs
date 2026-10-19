//! The data layer's side of the inputs (contracts.md section 6): the
//! `pipeline.*` relations and every plan's `query.<plan>.report.*` tables,
//! each one `scan` over all time through an `abt-wire` client on the data
//! socket. The `pipeline` class lands with the backtester's amendments;
//! this path is built against the request shapes of P-3.1 and tested with
//! the fixtures, which carry the same tables as JSON.

use std::path::Path;

use abt_wire::generated::abt::wire::{QueryForm, QueryRequest, Scan};
use abt_wire::rpc::{Client, WireError};
use abt_wire::transport::Connection;
use abt_wire::{ErrorCode, Layer, Message};
use anyhow::{Context, Result};

use crate::inputs::{Inputs, Offset};
use crate::Table;

pub const PIPELINE_RELATIONS: &[&str] = &["pipeline.document", "pipeline.hypothesis", "pipeline.submission", "pipeline.outcome", "pipeline.audit"];
pub const REPORT_TABLES: &[&str] = &[
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
];

/// A `scan` of `relation` over all time (P-3.2), as a table.
pub fn scan_request(relation: &str) -> Message {
    Message::QueryRequest(Box::new(QueryRequest {
        view: None,
        form: QueryForm::Scan(Box::new(Scan {
            relation: relation.to_string(),
            t0: i64::MIN,
            t1: i64::MAX,
            predicate: None,
            columns: None,
        })),
        receipt_filter: None,
        with_seq: false,
        explain: false,
        ticks: false,
    }))
}

/// Scans `relation`; `Ok(None)` when the service has no such relation (a
/// report table the plan did not write).
pub fn scan(client: &Client, relation: &str) -> Result<Option<Table>> {
    match client.request_table(scan_request(relation)) {
        Ok((_, batches)) => Ok(Some(Table::from_batches(&batches))),
        Err(WireError::Remote(e)) if matches!(e.code, ErrorCode::UnknownRelation | ErrorCode::NotFound) => Ok(None),
        Err(e) => Err(anyhow::anyhow!("{}: {}", relation, e)),
    }
}

/// Reads the `pipeline.*` relations and the report tables of every plan
/// the submissions in `inputs` name.
pub fn read(socket: &Path, inputs: &mut Inputs) -> Result<()> {
    let conn = Connection::connect(socket).with_context(|| socket.display().to_string())?;
    let client = Client::connect(conn, Layer::Front, "abt-report").context("the data service's hello")?;
    for relation in PIPELINE_RELATIONS {
        if let Some(t) = scan(&client, relation)? {
            inputs.offsets.push(Offset {
                input: relation.to_string(),
                detail: socket.display().to_string(),
                records: t.len() as u64,
            });
            inputs.pipeline.insert(relation.to_string(), t);
        }
    }
    let mut tables = 0u64;
    for plan in inputs.plan_hashes() {
        let mut by_name = std::collections::BTreeMap::new();
        for name in REPORT_TABLES {
            if let Some(t) = scan(&client, &format!("query.{}.report.{}", plan, name))? {
                by_name.insert(name.to_string(), t);
                tables += 1;
            }
        }
        inputs.reports.insert(plan, by_name);
    }
    inputs.offsets.push(Offset {
        input: "query.<plan>.report.*".into(),
        detail: format!("{} ({} plans)", socket.display(), inputs.reports.len()),
        records: tables,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_request_round_trips_the_wire() {
        let m = scan_request("query.ab12.report.summary");
        let bytes = abt_wire::convert::encode(m.clone());
        let back = abt_wire::convert::decode(&bytes).unwrap();
        assert_eq!(back, m);
    }
}

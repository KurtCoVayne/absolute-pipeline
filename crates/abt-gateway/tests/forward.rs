//! The forward scheduler (PL-7.3) over the in-process layers: a submission
//! with `H` in the middle of the synthetic data gets one reveal whose
//! window is `(H, last]` (exactly so with the amendment of PL-6.6 in the
//! query layer; under the pinned tree the gateway rewrites `after H` as
//! `trailing:Ny`, the embargo then starts a whole number of years before
//! the last bar, and the test says which it saw); a second pass with no
//! new data makes no reveal; the outcome is in `pipeline.outcome` and as
//! JSON in `--out`; the owner's `retire` writes a retired outcome and the
//! scheduler then leaves the submission alone; a tombstone on `control`
//! retires too.

mod support;

use serde_json::{json, Value};
use support::*;

use abt_gateway::forward::{self, Config};
use abt_gateway::provenance;

#[test]
fn a_submission_is_revealed_once_per_period_and_retired_on_command() {
    logs();
    let m = market(520, 400, 5);
    let src = momentum("idea_moc");
    let prog = program(&src, "idea_moc");
    let w = World::new(&m.ds, &prog, 2, Default::default());
    let token = w.agent_token("job-fwd", m.h);
    let plan = development_plan("idea_moc", m.h, m.first);
    let (status, r) = w.call("plan_run", &token, json!({ "plan": plan, "programs": programs("idea_moc", &src) }));
    assert_eq!(status, 200, "{}", r);
    let (plan_hash, program_hash) = (r["result"]["plan_hash"].as_str().unwrap().to_string(), r["result"]["program_hash"].as_str().unwrap().to_string());
    let (status, r) = w.call(
        "submit",
        &token,
        json!({
            "hypothesis_id": "ab".repeat(32),
            "program_hash": program_hash,
            "plan_hash": plan_hash,
            "program_text": src,
            "plan_text": plan,
            "forward_plan_text": forward_plan("idea_moc", m.h),
            "claims": [],
            "rationale": "",
            "insample": {},
        }),
    );
    assert_eq!(status, 200, "{}", r);
    let sid = r["result"]["submission_id"].as_str().unwrap().to_string();
    let out = tempfile::tempdir().unwrap();
    let cfg = Config {
        period: 7 * DAY,
        out: Some(out.path().to_path_buf()),
        brokers: None,
    };
    // The first pass: one reveal, over (H, last].
    let now = w.now + DAY;
    let revealed = forward::tick(&w.layers, &cfg, now).unwrap();
    assert_eq!(revealed.len(), 1, "{:?}", revealed);
    let r = &revealed[0];
    assert_eq!(r.submission_id, sid);
    assert_eq!((r.reveal, r.status.as_str()), (1, "forward"));
    assert!(
        r.forward_to <= m.last && r.forward_to > m.last - 5 * DAY,
        "the window ends at the last bar: {}",
        pipeline_wire::iso(r.forward_to)
    );
    // Whether the query layer took `holdout after H` shows in the window's
    // start: with the amendment it is the first bar after H; without it,
    // the gateway's `trailing:Ny` rewrite starts the embargo a whole year
    // before the last bar, at or before H.
    let exact = r.forward_from > m.h;
    if exact {
        assert!(r.forward_from <= m.h + 5 * DAY, "the window starts right after H: {}", pipeline_wire::iso(r.forward_from));
        println!(
            "the query layer takes `holdout after H`: the reveal's window is ({}, {}]",
            pipeline_wire::iso(m.h),
            pipeline_wire::iso(r.forward_to)
        );
    } else {
        println!(
            "the query layer lacks `holdout after H` (the pinned tree): the window [{}, {}] starts at the trailing year's embargo, before H {}",
            pipeline_wire::iso(r.forward_from),
            pipeline_wire::iso(r.forward_to),
            pipeline_wire::iso(m.h)
        );
    }
    // The outcome is in the data layer and in the out directory.
    let outcomes = provenance::scan_all(&w.layers, provenance::OUTCOME).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes.first("submission_id").unwrap(), &Value::String(sid.clone()));
    assert_eq!(outcomes.first("status").unwrap(), "forward");
    let metrics: Value = serde_json::from_str(outcomes.first("metrics").unwrap().as_str().unwrap()).unwrap();
    assert!(metrics["trials"].as_u64().is_some_and(|t| t >= 3), "the lineage's count: {}", metrics);
    assert!(metrics["returns"]["sharpe"].is_number(), "{}", metrics);
    assert!(metrics.get("dsr").is_some());
    let files: Vec<_> = std::fs::read_dir(out.path()).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).collect();
    assert_eq!(files.len(), 1);
    let written: Value = serde_json::from_str(&std::fs::read_to_string(files[0].path()).unwrap()).unwrap();
    assert_eq!(written["record"], "Outcome");
    assert_eq!(written["submission_id"], sid);
    // A second pass with no new data: nothing due.
    let again = forward::tick(&w.layers, &cfg, now + DAY).unwrap();
    assert!(again.is_empty(), "{:?}", again);
    assert_eq!(provenance::scan_all(&w.layers, provenance::OUTCOME).unwrap().len(), 1);
    // The owner retires it; the scheduler then leaves it alone.
    let retired = forward::retire(&w.layers, &cfg, &sid[..12], "the owner says so", now + 2 * DAY).unwrap();
    assert_eq!((retired.status.as_str(), retired.reveal), ("retired", 1));
    assert!(forward::retire(&w.layers, &cfg, &sid, "again", now + 3 * DAY).is_err());
    let latest = forward::latest_outcomes(&w.layers).unwrap();
    assert_eq!(latest[&sid].status, "retired");
    assert!(forward::tick(&w.layers, &cfg, now + 400 * DAY).unwrap().is_empty());
}

#[test]
fn a_tombstone_on_control_retires_the_submissions_of_its_document() {
    let m = market(400, 300, 6);
    let src = momentum("idea_moc");
    let prog = program(&src, "idea_moc");
    let w = World::new(&m.ds, &prog, 1, Default::default());
    let doc = "11".repeat(32);
    let hyp = "22".repeat(32);
    let orchestrator = w.token(&w.grant("orchestrator", m.h, &["provenance.write"]));
    let (status, r) = w.call(
        "provenance",
        &orchestrator,
        json!({ "hypotheses": [{ "record": "Hypothesis", "id": hyp, "doc_ids": [doc], "h": pipeline_wire::iso(m.h), "agent": "analyst", "r": pipeline_wire::iso(m.h) }] }),
    );
    assert_eq!(status, 200, "{}", r);
    let token = w.agent_token("job-tomb", m.h);
    let plan = development_plan("idea_moc", m.h, m.first);
    let (status, r) = w.call("plan_run", &token, json!({ "plan": plan, "programs": programs("idea_moc", &src) }));
    assert_eq!(status, 200, "{}", r);
    let (plan_hash, program_hash) = (r["result"]["plan_hash"].as_str().unwrap().to_string(), r["result"]["program_hash"].as_str().unwrap().to_string());
    let (status, r) = w.call(
        "submit",
        &token,
        json!({ "hypothesis_id": hyp, "program_hash": program_hash, "plan_hash": plan_hash, "program_text": src, "plan_text": plan, "forward_plan_text": forward_plan("idea_moc", m.h), "claims": [], "rationale": "", "insample": {} }),
    );
    assert_eq!(status, 200, "{}", r);
    let sid = r["result"]["submission_id"].as_str().unwrap().to_string();
    let out = tempfile::tempdir().unwrap();
    let tomb = pipeline_wire::json::render(&pipeline_wire::Record::Tombstone(Box::new(pipeline_wire::Tombstone {
        id: "33".repeat(32),
        subject: pipeline_wire::TombstoneSubject::Document,
        subject_id: doc.clone(),
        doc_id: Some(doc.clone()),
        reason: "deleted at the source".into(),
        r: w.now,
    })));
    std::fs::write(out.path().join("control.jsonl"), format!("{}\n", tomb)).unwrap();
    let cfg = Config {
        period: DAY,
        out: Some(out.path().to_path_buf()),
        brokers: None,
    };
    let revealed = forward::tick(&w.layers, &cfg, w.now).unwrap();
    assert_eq!(revealed.len(), 1);
    assert_eq!(revealed[0].status, "retired");
    assert!(revealed[0].retired_reason.as_deref().unwrap().contains("tombstone"));
    assert_eq!(forward::latest_outcomes(&w.layers).unwrap()[&sid].status, "retired");
    assert!(forward::tick(&w.layers, &cfg, w.now + DAY).unwrap().is_empty());
}

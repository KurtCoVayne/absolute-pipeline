//! The tool API's routes (contracts section 5) over the in-process layers:
//! every route answers in its shape through the HTTP face and straight
//! through the gateway; a token without the capability is 403, a missing
//! route 404, a bad body 400; the guards' refusals carry their codes; a
//! plan runs under the budgets and the queue, its reveal step revealed
//! once; `submit` validates as PL-6.4 and writes `pipeline.submission`;
//! `provenance` writes the documents and hypotheses; every request leaves
//! a row in `pipeline.audit`; `/metrics` exports the queue.

mod support;

use serde_json::{json, Value};
use support::*;

use abt_gateway::api::Config;
use abt_gateway::provenance;

fn world() -> (World, Market, String, String) {
    let m = market(300, 200, 7);
    let src = momentum("idea");
    let prog = program(&src, "idea");
    let w = World::new(&m.ds, &prog, 2, Config::default());
    (w, m, src, "idea".into())
}

fn ok(r: (u16, Value)) -> Value {
    assert_eq!(r.0, 200, "{}", r.1);
    r.1["result"].clone()
}

fn err(r: (u16, Value), status: u16) -> Value {
    assert_eq!(r.0, status, "{}", r.1);
    assert_eq!(r.1["ok"], false);
    r.1["error"].clone()
}

#[test]
fn the_routes_answer_in_their_shapes_over_http_and_in_process() {
    let (w, m, src, name) = world();
    let addr = serve_http(w.gateway.clone());
    let token = w.agent_token("job-routes", m.h);
    // health, without a token.
    let (status, body) = post(addr, "health", None, &json!({}));
    assert_eq!(status, 200, "{}", body);
    assert_eq!(body["result"]["data"], "ok");
    assert_eq!(body["result"]["kernels"], json!(["ok", "ok"]));
    assert_eq!(body["result"]["query"], "ok");
    // catalog: a table of the registry.
    let r = ok(post(addr, "catalog", Some(&token), &json!({ "class": "equity" })));
    assert!(r["columns"].as_array().unwrap().iter().any(|c| c == "id"), "{}", r);
    assert_eq!(r["rows"].as_array().unwrap().len(), SYMS.len() + 1);
    assert_eq!(r["truncated"], false);
    // schema: the class catalog's text.
    let r = ok(post(addr, "schema", Some(&token), &json!({ "class": "equity" })));
    let text = r["text"].as_str().unwrap();
    assert!(text.starts_with("class equity {"), "{}", text);
    assert!(text.contains("close("), "{}", text);
    assert!(text.contains("ticker(") && text.contains("instrument("), "{}", text);
    assert!(r["relations"].as_array().unwrap().iter().any(|x| x == "ticker"));
    // The registry's relations answer `query` by their bare names.
    let r = ok(post(addr, "query", Some(&token), &json!({ "form": { "kind": "scan", "relation": "ticker" } })));
    assert!(r["columns"].as_array().unwrap().iter().any(|c| c == "symbol"), "{}", r);
    assert_eq!(err(post(addr, "schema", Some(&token), &json!({ "class": "nothing" })), 400)["code"], "unknown_class");
    // resolve_symbols.
    let r = ok(post(addr, "resolve_symbols", Some(&token), &json!({ "symbols": ["AAA", "LATE", "NOPE"] })));
    let rows = r.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["symbol"], "AAA");
    assert!(rows[0]["abt_id"].is_number(), "{}", rows[0]);
    assert!(rows[0]["class"] == "equity" || rows[0]["class"] == "" || rows[0]["class"].is_null(), "{}", rows[0]);
    assert!(rows[2]["abt_id"].is_null());
    // query: a clamped series.
    let id = rows[0]["abt_id"].as_u64().unwrap();
    let r = ok(post(
        addr,
        "query",
        Some(&token),
        &json!({ "form": { "kind": "series", "relation": "equity.close", "identity": [id], "t0": "2022-01-01", "t1": "2030-01-01" } }),
    ));
    assert_eq!(r["r_max"], pipeline_wire::iso(m.h));
    let n = r["rows"].as_array().unwrap().len();
    assert!(n > 100 && n <= 201, "{}", n);
    // The same in process.
    let (status, body) = w.call("query", &token, json!({ "form": { "kind": "count", "relation": "equity.close", "predicate": format!("A = {}", id) } }));
    assert_eq!(status, 200, "{}", body);
    assert_eq!(body["result"]["rows"][0][0], json!(n));
    // check, contract, show.
    let r = ok(post(addr, "check", Some(&token), &json!({ "program": src, "name": name })));
    assert!(r["program_hash"].as_str().is_some_and(|h| h.len() == 64), "{}", r);
    assert_eq!(r["checks"], true);
    let r = ok(post(addr, "contract", Some(&token), &json!({ "program": src, "name": name })));
    assert!(r["clauses"].as_array().is_some_and(|c| !c.is_empty()));
    let r = ok(post(addr, "show", Some(&token), &json!({ "program": src, "name": name, "what": "program" })));
    assert!(r["text"].as_str().is_some_and(|t| t.contains("decide(")), "{}", r);
    // A program that does not check answers its diagnostics.
    let r = ok(post(
        addr,
        "check",
        Some(&token),
        &json!({ "program": src.replace("selected(A, T) :- bar(T)", "selected(A, T) :- nowhere(T)"), "name": name }),
    ));
    assert_eq!(r["checks"], false);
    assert!(r["diagnostics"].as_array().is_some_and(|d| !d.is_empty()), "{}", r);
    // The errors of the face.
    assert_eq!(post(addr, "nowhere", Some(&token), &json!({})).0, 404);
    assert_eq!(post(addr, "query", None, &json!({})).0, 403);
    let narrow = w.token(&w.grant("job-narrow", m.h, &["abt.catalog"]));
    assert_eq!(
        err(
            post(addr, "query", Some(&narrow), &json!({ "form": { "kind": "section", "relation": "equity.close", "t": "2022-03-01" } })),
            403
        )["code"],
        "forbidden"
    );
    assert_eq!(
        err(post(addr, "query", Some(&token), &json!({ "form": { "kind": "section", "relation": "equity.close" } })), 400)["code"],
        "malformed"
    );
    assert_eq!(
        err(
            post(addr, "query", Some(&token), &json!({ "form": { "kind": "section", "relation": "equity.nothing", "t": "2022-03-01" } })),
            400
        )["code"],
        "unknown_relation"
    );
    let (status, body) = post(addr, "query", Some(&token), &json!("not an object"));
    assert_eq!(status, 400, "{}", body);
    let (status, text) = get(addr, "/metrics");
    assert_eq!(status, 200);
    assert!(text.contains("abt_gateway_queue_depth 0"), "{}", text);
    assert!(text.contains("abt_gateway_requests_total{route=\"query\",outcome=\"ok\"}"), "{}", text);
    // Every request left an audit row.
    w.gateway.flush_audit();
    let audit = provenance::scan_all(&w.layers, provenance::AUDIT).unwrap();
    let kinds: Vec<&Value> = audit.column("kind");
    assert!(
        kinds.iter().any(|k| *k == "query") && kinds.iter().any(|k| *k == "catalog") && kinds.iter().any(|k| *k == "nowhere" || *k == "check"),
        "{:?}",
        kinds
    );
    let outcomes: Vec<&Value> = audit.column("outcome");
    assert!(outcomes.iter().any(|o| *o == "forbidden") && outcomes.iter().any(|o| *o == "ok"), "{:?}", outcomes);
    let jobs: Vec<&Value> = audit.column("job");
    assert!(jobs.iter().any(|j| *j == "job-routes"), "{:?}", jobs);
}

#[test]
fn a_plan_runs_under_the_guards_and_a_submission_is_recorded() {
    let (w, m, src, name) = world();
    let token = w.agent_token("job-plan", m.h);
    let plan = development_plan(&name, m.h, m.first);
    let body = json!({ "plan": plan, "programs": programs(&name, &src) });
    // plan_check: the hash and the steps.
    let (status, r) = w.call("plan_check", &token, body.clone());
    assert_eq!(status, 200, "{}", r);
    let checked = r["result"].clone();
    assert!(checked["hash"].as_str().is_some_and(|h| h.len() == 64), "{}", checked);
    assert_eq!(checked["kind"], "development");
    assert_eq!(checked["grid_points"], 2);
    assert!(checked["steps"].as_array().is_some_and(|s| s.len() >= 3), "{}", checked);
    // The guards, each with its code (contracts section 5).
    let refused = |plan: String, programs: Value, code: &str| {
        let (status, r) = w.call("plan_check", &token, json!({ "plan": plan, "programs": programs }));
        assert_eq!(status, 409, "{}", r);
        assert_eq!(r["error"]["code"], code, "{}", r);
    };
    refused(plan.replace("holdout    trailing:1y", "holdout    none"), programs(&name, &src), "holdout");
    refused(plan.replace(&abt_gateway::plan::render_h(m.h), "2022-06-01"), programs(&name, &src), "holdout");
    refused(plan.replace("money 0", "money 100"), programs(&name, &src), "budget");
    refused(
        plan.replace("n = 1, 2", "n = 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26"),
        programs(&name, &src),
        "grid",
    );
    refused(plan.clone(), programs(&name, &src.replace("execution equities_house", "execution equity_default")), "preset");
    refused(
        plan.clone(),
        programs(&name, &src.replace("execution equities_house", "execution equities_house with { lot whole; orders moo }")),
        "preset",
    );
    refused(plan.replace("idea.dsl", "other.dsl"), programs(&name, &src), "lineage");
    refused(
        forward_plan(&name, m.h).replace("steps      { run base; report main }", "steps { run base; reveal now of base; report main }"),
        programs(&name, &src),
        "reveal",
    );
    // plan_run: the runs, the report, the counters.
    let (status, r) = w.call("plan_run", &token, body.clone());
    assert_eq!(status, 200, "{}", r);
    let ran = r["result"].clone();
    assert_eq!(ran["plan_hash"], checked["hash"]);
    let runs = ran["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 3, "{}", ran);
    assert!(runs.iter().all(|x| x["run_id"].as_str().is_some_and(|i| i.len() == 64) && x["status"] == "completed"), "{}", ran);
    assert!(runs.iter().all(|x| x["decisions"].as_u64().is_some()), "{}", ran);
    assert_eq!(ran["trials"], 3);
    assert!(ran["report"]["summary"]["columns"].as_array().is_some(), "{}", ran["report"]);
    let state = w.gateway.jobs.state("job-plan");
    assert_eq!((state.plan_runs, state.grid_points), (1, 2));

    // plan_report of the job's plan, and of another's.
    let hash = checked["hash"].as_str().unwrap().to_string();
    let (status, r) = w.call("plan_report", &token, json!({ "plan_hash": hash }));
    assert_eq!(status, 200, "{}", r);
    assert!(r["result"]["summary"]["rows"].as_array().is_some_and(|x| !x.is_empty()), "{}", r);
    let other = w.agent_token("job-other", m.h);
    assert_eq!(w.call("plan_report", &other, json!({ "plan_hash": hash })).0, 403);
    // query_run of a run the job made, and of one it did not.
    let run_id = runs[0]["run_id"].as_str().unwrap().to_string();
    // The kernel's real answers (K-7.2): the run is re-created and held on
    // a gateway kernel with the same id, so no new trial.
    let (status, r) = w.call("query_run", &token, json!({ "run_id": run_id, "text": "show program" }));
    assert_eq!(status, 200, "{}", r);
    assert!(r["result"]["text"].as_str().is_some_and(|t| t.contains("decide(")), "{}", r);
    assert!(w.gateway.jobs.is_held("job-plan", &run_id));
    let (status, r) = w.call("query_run", &token, json!({ "run_id": run_id, "text": "coverage" }));
    assert_eq!(status, 200, "{}", r);
    assert!(
        r["result"]["rows"].as_array().is_some_and(|x| !x.is_empty()) || r["result"]["text"].as_str().is_some_and(|t| !t.is_empty()),
        "{}",
        r
    );
    let (status, r) = w.call("query_run", &token, json!({ "run_id": run_id, "text": "show contract" }));
    assert_eq!(status, 200, "{}", r);
    assert!(r["result"]["text"].as_str().is_some_and(|t| t.contains("pre.binding")), "{}", r);
    assert_eq!(w.call("query_run", &other, json!({ "run_id": run_id, "text": "coverage" })).0, 403);
    let (status, r) = w.call("plan_run", &token, body.clone());
    assert_eq!(status, 200, "{}", r);
    assert_eq!(r["result"]["trials"], 3, "the re-created run is the same trial: {}", r);
    // submit: the hashes name the plan the job ran; a second is refused.
    let program_hash = checked["program_hash"].as_str().unwrap().to_string();
    let submit = json!({
        "hypothesis_id": "ab".repeat(32),
        "program_hash": program_hash,
        "plan_hash": hash,
        "program_text": src,
        "plan_text": plan,
        "forward_plan_text": forward_plan(&name, m.h),
        "claims": ["survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after"],
        "rationale": "the author's point",
        "insample": { "sharpe": 1.0 },
    });
    let mut wrong = submit.clone();
    wrong["plan_hash"] = Value::String("cd".repeat(32));
    let (status, r) = w.call("submit", &token, wrong);
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "hashes");
    let (status, r) = w.call("submit", &token, submit.clone());
    assert_eq!(status, 200, "{}", r);
    let sid = r["result"]["submission_id"].as_str().unwrap().to_string();
    assert_eq!(sid.len(), 64);
    assert!(r["result"]["forward_plan_hash"].as_str().is_some_and(|h| h.len() == 64), "{}", r);
    assert_eq!(r["result"]["trials_used"], 3);
    assert!(r["result"]["forward_plan_text"].as_str().unwrap().contains("reveal forward of base"), "{}", r);
    let (status, r) = w.call("submit", &token, submit);
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "submitted");
    let stored = provenance::scan_all(&w.layers, provenance::SUBMISSION).unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored.first("id").unwrap(), &Value::String(sid));
    assert_eq!(stored.first("trials_used").unwrap(), &json!(3));
    // The plan budget: two more executions, then out_of_budget(plan_runs).
    assert_eq!(w.gateway.jobs.state("job-plan").plan_runs, 2);
    for _ in 0..2 {
        assert_eq!(w.call("plan_run", &token, body.clone()).0, 200);
    }
    let (status, r) = w.call("plan_run", &token, body.clone());
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "plan_runs");
}

#[test]
fn provenance_is_written_by_the_orchestrators_token_only() {
    let (w, m, _, _) = world();
    let agent = w.agent_token("job-p", m.h);
    let docs = json!({
        "documents": [{ "record": "Document", "id": "11".repeat(32), "source": "X", "author": { "id": "1", "handle": "someone" }, "p": "2026-09-18T19:12:36Z", "r": "2026-09-18T19:12:40Z", "text": "a post" }],
        "hypotheses": [{ "record": "Hypothesis", "id": "22".repeat(32), "doc_ids": ["11".repeat(32)], "h": "2026-09-18T19:12:36Z", "agent": "analyst", "r": "2026-09-18T20:00:00Z" }],
    });
    assert_eq!(w.call("provenance", &agent, docs.clone()).0, 403);
    let orchestrator = w.token(&w.grant("orchestrator", m.h, &["provenance.write"]));
    let (status, r) = w.call("provenance", &orchestrator, docs);
    assert_eq!(status, 200, "{}", r);
    assert_eq!(r["result"]["written"], 2);
    let d = provenance::scan_all(&w.layers, provenance::DOCUMENT).unwrap();
    assert_eq!(d.first("doc_id").unwrap(), &Value::String("11".repeat(32)));
    assert_eq!(d.first("author").unwrap(), "someone");
    assert_eq!(d.first("size").unwrap(), &json!(6));
    let h = provenance::scan_all(&w.layers, provenance::HYPOTHESIS).unwrap();
    assert_eq!(h.first("id").unwrap(), &Value::String("22".repeat(32)));
    assert_eq!(h.first("h").unwrap(), "2026-09-18T19:12:36Z");
}

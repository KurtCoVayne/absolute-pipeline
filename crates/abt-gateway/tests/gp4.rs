//! GP-4, the honesty property (PL-8.3; Proposition PL-4.2): a token with
//! `H` cannot read a row received after `H` through any query form of
//! P-3.2, in process and through the tool API, whatever the request asks
//! (an `r_max` past `H`, a window past the data's end, any identity, any
//! predicate); `count` and `exists` agree with the clamped `scan`; and the
//! guards refuse with their codes: a plan without the required hold-out,
//! beyond its budgets, off the house presets, with a program outside the
//! lineage; `plan_reveal` from an agent (no route; a forged request to the
//! query layer's route name is 404); a `submit` whose hashes name a plan
//! the job did not run; a second `submit`.
//!
//! The market is the synthetic daily equity market with a name whose bars
//! in the ten sessions before `H` were received a week after it (late
//! rows, `r > H` with `t ≤ H`) and bars after `H` received as they
//! happened. `chain` is skipped: the synthetic market has no options.

mod support;

use std::net::SocketAddr;
use std::sync::OnceLock;

use abt_wire::generated::abt::wire::ReceiptFilter;
use proptest::prelude::*;
use serde_json::{json, Value};
use support::*;

struct Fixture {
    world: World,
    market: Market,
    addr: SocketAddr,
    token: String,
    ids: Vec<u64>,
    src: String,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let market = support::market(260, 150, 21);
        let src = momentum("idea");
        let prog = program(&src, "idea");
        let world = World::new(&market.ds, &prog, 1, Default::default());
        let addr = serve_http(world.gateway.clone());
        let token = world.agent_token("gp4", market.h);
        let (status, r) = post(addr, "resolve_symbols", Some(&token), &json!({ "symbols": SYMS.iter().chain(["LATE"].iter()).collect::<Vec<_>>() }));
        assert_eq!(status, 200, "{}", r);
        let ids: Vec<u64> = r["result"].as_array().unwrap().iter().filter_map(|x| x["abt_id"].as_u64()).collect();
        assert_eq!(ids.len(), SYMS.len() + 1);
        assert_eq!(r["result"][SYMS.len()]["symbol"], "LATE");
        Fixture { world, market, addr, token, ids, src }
    })
}

const KINDS: [&str; 8] = ["point", "latest", "series", "section", "scan", "exists", "count", "distinct"];
const RELATIONS: [&str; 3] = ["equity.close", "equity.volume", "equity.listed"];

/// A random query of P-3.2's forms: its JSON body for the `query` route.
fn query_body(f: &Fixture) -> impl Strategy<Value = Value> {
    let (first, last, h) = (f.market.first, f.market.last, f.market.h);
    let ids = f.ids.clone();
    let times = (first - 30 * DAY)..(last + 60 * DAY);
    (
        0..KINDS.len(),
        0..RELATIONS.len(),
        prop::option::of(prop_oneof![prop::sample::select(ids), 1u64..1000]),
        times.clone(),
        times.clone(),
        times,
        prop::option::of(prop_oneof![
            Just("P > 50".to_string()),
            Just("P <= 50 or V >= 1000".to_string()),
            (1u64..8).prop_map(|a| format!("A = {}", a)),
            (1u64..8).prop_map(|a| format!("A in ({}, {})", a, a + 1)),
            Just("A is null".to_string()),
        ]),
        prop::option::of(prop_oneof![Just(h - 40 * DAY), Just(h), Just(h + 30 * DAY), Just(i64::MAX / 2)]),
    )
        .prop_map(move |(k, rel, id, t, a, b, pred, r_max)| {
            let kind = KINDS[k];
            let relation = RELATIONS[rel];
            let (t0, t1) = if a <= b { (a, b) } else { (b, a) };
            let mut form = json!({ "kind": kind, "relation": relation });
            match kind {
                "point" | "section" => form["t"] = Value::from(t),
                "latest" => form["t_max"] = Value::from(t),
                _ => {
                    form["t0"] = Value::from(t0);
                    form["t1"] = Value::from(t1);
                }
            }
            if matches!(kind, "point" | "latest" | "series") {
                if let Some(i) = id {
                    form["identity"] = json!([i]);
                }
            }
            if matches!(kind, "scan" | "exists" | "count") {
                if let Some(p) = pred {
                    if relation != "equity.listed" || !p.contains('P') && !p.contains('V') {
                        form["predicate"] = Value::String(p);
                    }
                }
            }
            if kind == "distinct" {
                form["column"] = Value::String("A".into());
            }
            let mut body = json!({ "form": form });
            if let Some(r) = r_max {
                body["r_max"] = Value::from(r);
            }
            body
        })
}

/// Every `r` of a table's rows, nanoseconds.
fn receipts(t: &Value) -> Vec<i64> {
    let Some(i) = t["columns"].as_array().and_then(|c| c.iter().position(|x| x == "r")) else {
        return vec![];
    };
    t["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row.get(i).and_then(Value::as_str).and_then(|s| pipeline_wire::parse_iso(s).ok()))
        .collect()
}

/// The clamped scan the gateway's answer must agree with, straight through
/// the layers: every row of `relation` over `[t0, t1]` under `predicate`,
/// received by `h` (the reference of the property).
fn reference_scan(f: &Fixture, body: &Value) -> Vec<Vec<Value>> {
    let mut form = body["form"].clone();
    form["kind"] = Value::String("scan".into());
    let mut req = abt_gateway::query::request(abt_gateway::query::form(&form).unwrap());
    let asked = body.get("r_max").and_then(Value::as_i64).unwrap_or(i64::MAX);
    req.receipt_filter = Some(Box::new(ReceiptFilter { r_max: Some(asked.min(f.market.h)) }));
    let (_, batches) = f.world.layers.query(req).unwrap();
    abt_gateway::table::Table::of(&batches, usize::MAX).rows
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, max_shrink_iters: 200, ..ProptestConfig::default() })]

    #[test]
    fn no_answer_holds_a_row_received_after_h(body in query_body(fixture())) {
        let f = fixture();
        let h = f.market.h;
        for (how, (status, answer)) in [("http", post(f.addr, "query", Some(&f.token), &body)), ("in-process", f.world.call("query", &f.token, body.clone()))] {
            prop_assert!(status == 200 || status == 400, "{}: {} {}", how, status, answer);
            if status != 200 {
                continue;
            }
            let t = &answer["result"];
            let asked = body.get("r_max").and_then(Value::as_i64).unwrap_or(i64::MAX);
            prop_assert_eq!(t["r_max"].as_str().map(|s| pipeline_wire::parse_iso(s).unwrap()), Some(asked.min(h)), "{}: the clamp is min(asked, H)", how);
            prop_assert_eq!(t["h"].as_str().map(|s| pipeline_wire::parse_iso(s).unwrap()), Some(h), "{}", how);
            for r in receipts(t) {
                prop_assert!(r <= h, "{}: a row received at {} is after H {} in {}", how, pipeline_wire::iso(r), pipeline_wire::iso(h), body);
            }
            let kind = body["form"]["kind"].as_str().unwrap();
            if matches!(kind, "count" | "exists" | "distinct") {
                let reference = reference_scan(f, &body);
                match kind {
                    "count" => prop_assert_eq!(t["rows"][0][0].as_u64(), Some(reference.len() as u64), "{}: count disagrees with the clamped scan for {}", how, body),
                    "exists" => prop_assert_eq!(t["rows"][0][0].as_bool(), Some(!reference.is_empty()), "{}: exists disagrees with the clamped scan for {}", how, body),
                    _ => {
                        let seen: Vec<&Value> = t["rows"].as_array().unwrap().iter().filter_map(|r| r.get(0)).collect();
                        let in_scan: Vec<&Value> = reference.iter().filter_map(|r| r.first()).collect();
                        prop_assert!(seen.iter().all(|v| in_scan.contains(v)), "{}: a distinct value outside the clamped scan for {}", how, body);
                    }
                }
            }
        }
    }
}

#[test]
fn the_late_rows_are_invisible_and_the_series_stops_at_h() {
    let f = fixture();
    // The store's id of LATE (the dataset's own index is another thing).
    let late = f.ids[SYMS.len()];
    let h = f.market.h;
    // Every late row has t ≤ H and r > H: none is answered, through any
    // form, however the request asks.
    for (kind, extra) in [
        ("series", json!({ "t0": f.market.first - DAY, "t1": f.market.last + DAY })),
        ("scan", json!({ "t0": f.market.first - DAY, "t1": f.market.last + DAY, "predicate": format!("A = {}", late) })),
    ] {
        let mut form = json!({ "kind": kind, "relation": "equity.close", "identity": [late] });
        for (k, v) in extra.as_object().unwrap() {
            form[k] = v.clone();
        }
        let (status, r) = post(f.addr, "query", Some(&f.token), &json!({ "form": form, "r_max": i64::MAX / 2 }));
        assert_eq!(status, 200, "{}", r);
        let rows = r["result"]["rows"].as_array().unwrap();
        let ti = r["result"]["columns"].as_array().unwrap().iter().position(|c| c == "T").unwrap();
        assert!(receipts(&r["result"]).iter().all(|x| *x <= h));
        assert!(rows.iter().all(|row| pipeline_wire::parse_iso(row[ti].as_str().unwrap()).unwrap() <= h));
        // The ten late sessions before H are missing: the series has a hole.
        let before_h: Vec<&Value> = rows.iter().filter(|row| pipeline_wire::parse_iso(row[ti].as_str().unwrap()).unwrap() <= h).collect();
        assert_eq!(before_h.len(), 151 - f.market.late.len(), "{} rows before H, {} late", before_h.len(), f.market.late.len());
    }
    // A `latest` at a time after H answers the last bar received by H.
    let (status, r) = post(
        f.addr,
        "query",
        Some(&f.token),
        &json!({ "form": { "kind": "latest", "relation": "equity.close", "identity": [late], "t_max": f.market.last + DAY } }),
    );
    assert_eq!(status, 200, "{}", r);
    let ti = r["result"]["columns"].as_array().unwrap().iter().position(|c| c == "T").unwrap();
    let t = pipeline_wire::parse_iso(r["result"]["rows"][0][ti].as_str().unwrap()).unwrap();
    assert!(t < h - 10 * DAY, "the latest bar received by H is before the late sessions: {}", pipeline_wire::iso(t));
    println!("chain: skipped, the synthetic market has no options");
}

#[test]
fn the_guards_refuse_with_their_codes() {
    let f = fixture();
    let (h, first) = (f.market.h, f.market.first);
    let token = f.world.agent_token("gp4-guards", h);
    let plan = development_plan("idea", h, first);
    let refused = |plan: String, programs: Value, code: &str| {
        for route in ["plan_check", "plan_run"] {
            let (status, r) = post(f.addr, route, Some(&token), &json!({ "plan": plan, "programs": programs }));
            assert_eq!(status, 409, "{}: {}", route, r);
            assert_eq!(r["error"]["code"], code, "{}: {}", route, r);
        }
    };
    // Without the required hold-out: none, a block hold-out, a window
    // ending elsewhere, `after` another time.
    refused(plan.replace("holdout    trailing:1y", "holdout    none"), programs("idea", &f.src), "holdout");
    refused(plan.replace("holdout    trailing:1y", "holdout    blocks:2:2mo"), programs("idea", &f.src), "holdout");
    refused(plan.replace(&abt_gateway::plan::render_h(h), "2022-06-01"), programs("idea", &f.src), "holdout");
    refused(forward_plan("idea", h).replace(&abt_gateway::plan::render_h(h), "2022-06-01"), programs("idea", &f.src), "holdout");
    refused(plan.replace("money 0", "money 1"), programs("idea", &f.src), "budget");
    // Beyond the grid budget, then beyond the plan-execution budget.
    let grid: Vec<String> = (1..=26).map(|i| i.to_string()).collect();
    refused(plan.replace("n = 1, 2", &format!("n = {}", grid.join(", "))), programs("idea", &f.src), "grid");
    let small = f.world.token(&abt_gateway::Grant {
        plan_runs: 1,
        ..f.world.grant("gp4-one-run", h, AGENT_CAPS)
    });
    let body = json!({ "plan": plan.replace("grid       { n = 1, 2 }", "grid       { n = 2 }"), "programs": programs("idea", &f.src) });
    let (status, r) = post(f.addr, "plan_run", Some(&small), &body);
    assert_eq!(status, 200, "{}", r);
    let (status, r) = post(f.addr, "plan_run", Some(&small), &body);
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "plan_runs");
    // Off the house presets.
    refused(plan.clone(), programs("idea", &f.src.replace("execution equities_house", "execution equity_default")), "preset");
    refused(plan.clone(), programs("idea", &f.src.replace("execution equities_house", "execution { price close }")), "preset");
    // A program outside the lineage: none of the programs sent is the
    // plan's; and, with a lineage in the token, a program unlike its members.
    refused(plan.replace("idea.dsl", "elsewhere.dsl"), programs("idea", &f.src), "lineage");
    let other = f
        .src
        .replace("strategy idea", "strategy unrelated")
        .replace("M = P1 / P0 - 1", "M = P0 / P1 - 1")
        .replace("N = count(A) over (selected(A, T))", "N = count(A) over (universe(A, T))");
    let foreign = f.world.token(&abt_gateway::Grant {
        lineage: "ab".repeat(32),
        ..f.world.grant("gp4-lineage", h, AGENT_CAPS)
    });
    let (status, r) = post(
        f.addr,
        "plan_check",
        Some(&foreign),
        &json!({ "plan": plan.replace("idea.dsl", "unrelated.dsl"), "programs": programs("unrelated", &other) }),
    );
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "lineage");
    // plan_reveal: no route; a forged request to the query layer's route
    // name is 404, with an agent's token or none.
    assert_eq!(
        post(f.addr, "plan_reveal", Some(&token), &json!({ "plan": plan, "programs": programs("idea", &f.src), "step": "oos" })).0,
        404
    );
    assert_eq!(post(f.addr, "plan_reveal", None, &json!({})).0, 404);
    assert_eq!(post(f.addr, "reveal", Some(&token), &json!({})).0, 404);
    assert!(!abt_gateway::api::ROUTES.contains(&"plan_reveal"));
    // submit: hashes of a plan the job did not run; then a second submit.
    let (status, r) = post(f.addr, "plan_run", Some(&token), &json!({ "plan": plan, "programs": programs("idea", &f.src) }));
    assert_eq!(status, 200, "{}", r);
    let (plan_hash, program_hash) = (r["result"]["plan_hash"].as_str().unwrap().to_string(), r["result"]["program_hash"].as_str().unwrap().to_string());
    let submit = |plan_hash: &str, program_hash: &str| {
        json!({
            "hypothesis_id": "ab".repeat(32),
            "program_hash": program_hash,
            "plan_hash": plan_hash,
            "program_text": f.src,
            "plan_text": plan,
            "forward_plan_text": forward_plan("idea", h),
            "claims": [],
            "rationale": "",
            "insample": {},
        })
    };
    let (status, r) = post(f.addr, "submit", Some(&token), &submit(&"cd".repeat(32), &program_hash));
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "hashes");
    let (status, r) = post(f.addr, "submit", Some(&token), &submit(&plan_hash, &"cd".repeat(32)));
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "hashes");
    let (status, r) = post(f.addr, "submit", Some(&token), &submit(&plan_hash, &program_hash));
    assert_eq!(status, 200, "{}", r);
    let (status, r) = post(f.addr, "submit", Some(&token), &submit(&plan_hash, &program_hash));
    assert_eq!(status, 409, "{}", r);
    assert_eq!(r["error"]["code"], "submitted");
}

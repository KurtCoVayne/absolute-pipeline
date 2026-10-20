//! PL-6.8's gateway-side rows, measured over the in-process layers: the
//! tool API's round trip for `catalog` and `query` (p50 and p99 over the
//! loopback HTTP face, the token verified each time), and the kernel
//! queue under eight concurrent fake jobs on two kernels (the depth seen,
//! the fair share, nothing lost, nothing `busy` below the configured
//! depth; `busy` past it). The numbers are printed for the report; the
//! assertions are the bounds the plan sets.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use support::*;

fn percentile(xs: &mut [f64], p: f64) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[((xs.len() as f64 - 1.0) * p).round() as usize]
}

#[test]
fn round_trips_and_the_queue_under_eight_jobs() {
    let m = market(260, 150, 3);
    let src = momentum("idea_moc");
    let prog = program(&src, "idea_moc");
    let w = World::new(&m.ds, &prog, 2, abt_gateway::api::Config { max_depth: 4, ..Default::default() });
    let addr = serve_http(w.gateway.clone());
    let token = w.agent_token("load", m.h);
    let (status, r) = post(addr, "resolve_symbols", Some(&token), &json!({ "symbols": ["AAA"] }));
    assert_eq!(status, 200, "{}", r);
    let id = r["result"][0]["abt_id"].as_u64().unwrap();
    // Warm up, then measure.
    for _ in 0..10 {
        assert_eq!(post(addr, "catalog", Some(&token), &json!({ "class": "equity" })).0, 200);
    }
    let n = 200;
    let mut catalog = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        let (status, _) = post(addr, "catalog", Some(&token), &json!({ "class": "equity" }));
        catalog.push(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(status, 200);
    }
    let mut query = Vec::with_capacity(n);
    for i in 0..n {
        let t = Instant::now();
        let (status, _) = post(
            addr,
            "query",
            Some(&token),
            &json!({ "form": { "kind": "series", "relation": "equity.close", "identity": [id], "t0": m.first + (i as i64 % 50) * DAY, "t1": m.last } }),
        );
        query.push(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(status, 200);
    }
    let (c50, c99) = (percentile(&mut catalog, 0.5), percentile(&mut catalog, 0.99));
    let (q50, q99) = (percentile(&mut query, 0.5), percentile(&mut query, 0.99));
    println!("PL-6.8 catalog round trip over HTTP: p50 {:.2} ms, p99 {:.2} ms ({} requests)", c50, c99, n);
    println!("PL-6.8 query (series, ~200 rows) round trip over HTTP: p50 {:.2} ms, p99 {:.2} ms ({} requests)", q50, q99, n);
    assert!(c99 < 250.0 && q99 < 250.0, "a round trip is milliseconds, not seconds: {} {}", c99, q99);

    // Eight fake jobs, each one plan execution of a two-point grid, on two
    // kernels: at most two in flight, the others queued; everything served.
    let plan = Arc::new(development_plan("idea_moc", m.h, m.first));
    let max_depth = Arc::new(AtomicUsize::new(0));
    let gateway = w.gateway.clone();
    let watcher = {
        let (g, d) = (gateway.clone(), max_depth.clone());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s = stop.clone();
        let h = std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                d.fetch_max(g.queue.depth(), Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        (stop, h)
    };
    let started = Instant::now();
    let handles: Vec<_> = (0..8)
        .map(|j| {
            // Each job its own plan (another grid), as real jobs are: one
            // plan's runs are one run each, which a kernel runs once at a time.
            let own = plan.replace("n = 1, 2", &format!("n = {}, {}", 1 + j % 4, 2 + j % 4));
            let (w_token, src) = (w.agent_token(&format!("fake-{}", j), m.h), src.clone());
            std::thread::spawn(move || {
                let t = Instant::now();
                let (status, r) = post(addr, "plan_run", Some(&w_token), &json!({ "plan": own, "programs": programs("idea_moc", &src) }));
                if status != 200 && r["error"]["code"] != "busy" {
                    println!("fake-{}: {} {}", j, status, r);
                }
                (status, r["error"]["code"].as_str().unwrap_or("").to_string(), t.elapsed().as_secs_f64())
            })
        })
        .collect();
    let results: Vec<(u16, String, f64)> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    watcher.0.store(true, Ordering::Relaxed);
    watcher.1.join().unwrap();
    let wall = started.elapsed().as_secs_f64();
    let served = results.iter().filter(|r| r.0 == 200).count();
    let busy = results.iter().filter(|r| r.1 == "busy").count();
    let mut waits: Vec<f64> = results.iter().filter(|r| r.0 == 200).map(|r| r.2).collect();
    println!(
        "PL-6.8 queue, 8 jobs on 2 kernels (max depth 4): {} served, {} busy, {} other; deepest {}; wall {:.2} s; per-job p50 {:.2} s, max {:.2} s; permits in flight now {}",
        served,
        busy,
        8 - served - busy,
        max_depth.load(Ordering::Relaxed),
        wall,
        percentile(&mut waits, 0.5),
        waits.iter().cloned().fold(0.0, f64::max),
        gateway.queue.in_flight()
    );
    // The first job alone takes both permits (its grid spreads over the
    // pool), the next four wait, the rest are busy: at least 1 + depth served.
    assert!(served >= 1 + 4, "{:?}", results);
    assert_eq!(served + busy, 8, "{:?}", results);
    assert!(max_depth.load(Ordering::Relaxed) <= 4);
    assert_eq!(gateway.queue.in_flight(), 0);
    assert_eq!(gateway.queue.served_total() as usize, served);
    let text = gateway.metrics_text();
    assert!(text.contains(&format!("abt_gateway_queue_busy_total {}", busy)), "{}", text);
}

//! The gateway's tests run the three layers in this process through the
//! channel transport (PL-7.1's decision; A-D3, A-D5): a data service over
//! a synthetic market with receipt times, a pool of kernel services
//! reaching it, a query service over both, and the gateway's `Layers` over
//! the three, with the frames the deployment carries. The tool API is
//! served on a loopback port and spoken to over plain HTTP/1.1.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;

use abt_data::service::DataService;
use abt_data::Store;
use abt_gateway::api::{Config, Gateway};
use abt_gateway::token::{mint, Grant, Verifier};
use abt_gateway::Layers;
use abt_kernel::check::{check_program, Program};
use abt_kernel::kernel::service::KernelService;
use abt_kernel::kernel::{DataClient, Dataset, Sym, Value};
use abt_query::service::QueryService;
use biscuit_auth::KeyPair;
use serde_json::Value as Json;

/// The gateway's logs on stderr (once; `RUST_LOG` filters).
pub fn logs() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .with_test_writer()
        .try_init();
}

/// A day's instant from its civil date.
pub fn day(y: i64, m: u32, d: u32) -> i64 {
    abt_kernel::data::business_days((y, m, d), 1)[0]
}

pub const DAY: i64 = 86_400 * 1_000_000_000;

/// The names of the market.
pub const SYMS: [&str; 4] = ["AAA", "BBB", "CCC", "SPY"];

/// A daily equity market of `days` business days from 2022-01-03 with, on
/// top of the synthetic bars, late rows: bars of `LATE` whose event time
/// is at or before `h` but whose receipt is after it (`late_after_h` of
/// them), and bars after `h` received when they happened. `h` is the
/// instant of the bar `h_index` days in.
pub struct Market {
    pub ds: Dataset,
    pub h: i64,
    pub first: i64,
    pub last: i64,
    /// The late rows' (entity, t, r).
    pub late: Vec<(u64, i64, i64)>,
    pub late_sym: Sym,
}

pub fn market(days: usize, h_index: usize, seed: u64) -> Market {
    let mut ds = abt_kernel::data::synthetic_daily(&SYMS, (2022, 1, 3), days, seed);
    let bars = abt_kernel::data::business_days((2022, 1, 3), days);
    let h = bars[h_index] + 23 * 3_600 * 1_000_000_000;
    // One more name whose bars in the ten sessions before H arrive a week
    // after H: received after the horizon, event-timed before it.
    let late_sym = ds.add_security("LATE", "LATE", bars[0], None);
    let mut late = Vec::new();
    for (i, t) in bars.iter().enumerate() {
        let price = 50.0 + (i as f64) * 0.1;
        let received = if i + 10 >= h_index && i <= h_index { Some(h + 7 * DAY) } else { None };
        for rel in ["open", "high", "low", "close"] {
            let tuple = vec![Value::Equity(late_sym), Value::Time(*t), Value::Num(price)];
            match received {
                Some(r) => ds.add_available(rel, tuple, r),
                None => ds.add(rel, tuple),
            }
        }
        let vol = vec![Value::Equity(late_sym), Value::Time(*t), Value::Num(1_000.0)];
        match received {
            Some(r) => {
                ds.add_available("volume", vol, r);
                late.push((late_sym as u64, *t, r));
            }
            None => ds.add("volume", vol),
        }
        ds.add("listed", vec![Value::Equity(late_sym), Value::Time(*t)]);
    }
    ds.derive_tickers();
    Market {
        ds,
        h,
        first: bars[0],
        last: bars[days - 1],
        late,
        late_sym,
    }
}

/// A program checked against the catalog, the standard library and the
/// house presets (what the gateway sends with every program).
pub fn program(src: &str, name: &str) -> Program {
    let mut ws = abt_tests::base_workspace();
    ws.add_source(abt_gateway::HOUSE_PRESETS).unwrap();
    ws.add_source(src).unwrap_or_else(|e| panic!("parse error at {}: {}", e.span, e.message));
    let (p, diags) = check_program(&ws, name);
    p.unwrap_or_else(|| panic!("{} should check:\n{}", name, abt_tests::text(&diags)))
}

/// The momentum book of the old study under the equities house preset.
pub fn momentum(name: &str) -> String {
    abt_tests::momentum(name, "equities_house with { orders moc }")
}

/// A data service holding the market (the front's `serve_dataset`,
/// repeated here because the gateway links no front).
pub fn data_service(ds: &Dataset, prog: &Program) -> Arc<DataService> {
    let mut store = Store::temporary("gateway").unwrap();
    let mut attributes = BTreeMap::new();
    attributes.insert("calendar".to_string(), prog.base.to_string().trim_start_matches('@').to_string());
    if !ds.unreviewed {
        attributes.insert("reviewed".to_string(), "complete".to_string());
    }
    let schemas = store.registry.schemas(None);
    let class = prog.class.clone();
    let (sec, terms, relations) = abt_kernel::kernel::bind::keyed_tables(ds, &|name| abt_kernel::kernel::bind::schema_in(&schemas, &class, name)).unwrap();
    let securities = abt_data::adapter::securities_of(&sec).unwrap();
    let terms = abt_data::adapter::terms_of(&terms).unwrap();
    abt_data::adapter::ingest_keyed_with(&mut store, "dataset", &securities, &terms, &relations, "dataset", &attributes, &abt_data::adapter::hot_bars(&relations)).unwrap();
    for s in schemas.iter().filter(|s| s.class == class || s.class == "series") {
        abt_data::adapter::ingest_empty(&mut store, &s.name, "dataset", &attributes).unwrap();
    }
    DataService::new(store)
}

/// The three services in this process and the gateway's layers over them.
pub struct Services {
    pub data: Arc<DataService>,
    pub kernels: Vec<Arc<KernelService>>,
    pub query: Arc<QueryService>,
}

impl Services {
    pub fn new(ds: &Dataset, prog: &Program, n_kernels: usize) -> Services {
        let data = data_service(ds, prog);
        let kernels: Vec<Arc<KernelService>> = (0..n_kernels)
            .map(|_| {
                let d = data.clone();
                KernelService::new(Arc::new(move || DataClient::connect(abt_data::service::inproc(&d))))
            })
            .collect();
        let d = data.clone();
        let ks = kernels.clone();
        let query = QueryService::new(
            Arc::new(move || Ok(abt_data::service::inproc(&d))),
            Arc::new(move || Ok(ks.iter().map(abt_kernel::kernel::service::inproc).collect())),
        );
        Services { data, kernels, query }
    }

    /// The gateway's face over the services, through in-process channels.
    pub fn layers(&self) -> Arc<Layers> {
        let data = abt_data::service::inproc(&self.data);
        let kernels = self.kernels.iter().map(abt_kernel::kernel::service::inproc).collect();
        let query = abt_query::service::inproc(&self.query);
        Arc::new(Layers::connect(data, kernels, query, abt_tests::library_sources()).unwrap())
    }
}

/// A gateway with a fresh root key and a clock of the tests' choosing.
pub struct World {
    pub services: Services,
    pub layers: Arc<Layers>,
    pub gateway: Arc<Gateway>,
    pub root: KeyPair,
    pub now: i64,
}

impl World {
    pub fn new(ds: &Dataset, prog: &Program, n_kernels: usize, config: Config) -> World {
        let services = Services::new(ds, prog, n_kernels);
        let layers = services.layers();
        let root = KeyPair::new();
        let now = 1_800_000_000_000_000_000i64; // 2027-01-15
        let gateway = Arc::new(Gateway::new(layers.clone(), Verifier::new(root.public()), config).with_clock(move || now));
        World { services, layers, gateway, root, now }
    }

    pub fn grant(&self, job: &str, h: i64, caps: &[&str]) -> Grant {
        Grant {
            job: job.into(),
            agent: "quant".into(),
            caps: caps.iter().map(|s| s.to_string()).collect(),
            h,
            lineage: String::new(),
            plan_runs: 4,
            grid_points: 25,
            expires: self.now + 3_600 * 1_000_000_000,
        }
    }

    pub fn token(&self, grant: &Grant) -> String {
        mint(&self.root, grant).unwrap()
    }

    /// An agent's token with every agent capability.
    pub fn agent_token(&self, job: &str, h: i64) -> String {
        self.token(&self.grant(job, h, AGENT_CAPS))
    }

    /// A request straight to the gateway (no HTTP).
    pub fn call(&self, route: &str, token: &str, body: Json) -> (u16, Json) {
        self.gateway.handle(route, Some(&format!("Biscuit {}", token)), body.to_string().as_bytes())
    }
}

pub const AGENT_CAPS: &[&str] = &["abt.catalog", "abt.query", "abt.check", "abt.plan", "abt.report", "abt.query_run", "abt.submit"];

/// The tool API served on a loopback port (a tokio runtime on a thread of
/// its own); the address.
pub fn serve_http(gateway: Arc<Gateway>) -> SocketAddr {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            abt_gateway::api::serve(gateway, listener).await.unwrap();
        });
    });
    rx.recv().unwrap()
}

/// `POST /v1/<route>` over HTTP/1.1 with the token; the status and the
/// body as JSON.
pub fn post(addr: SocketAddr, route: &str, token: Option<&str>, body: &Json) -> (u16, Json) {
    let payload = body.to_string();
    let mut req = format!(
        "POST /v1/{} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        route,
        addr,
        payload.len()
    );
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Biscuit {}\r\n", t));
    }
    req.push_str("\r\n");
    req.push_str(&payload);
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let json: Json = serde_json::from_str(body.trim()).unwrap_or_else(|_| Json::String(body.to_string()));
    (status, json)
}

/// `GET <path>` over HTTP/1.1; the status and the body's text.
pub fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, addr);
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, body.to_string())
}

/// The house template's development plan for a program (guideline
/// section 15, cut to the synthetic market): a grid of `n`, the trailing
/// hold-out, the window ending at `H`, no money.
pub fn development_plan(name: &str, h: i64, first: i64) -> String {
    format!(
        "study idea_{name} {{\n  program    {name}.dsl\n  grid       {{ n = 1, 2 }}\n  holdout    trailing:1y\n  window     {first} .. {h}\n  budget     {{ money 0; bytes 2G; requests 0 }}\n  objective  sharpe\n  steps {{\n    run    base\n    grid   sens\n    report main\n  }}\n}}\n",
        name = name,
        first = abt_query::time::format_timestamp(first),
        h = abt_gateway::plan::render_h(h),
    )
}

/// The house template's forward plan.
pub fn forward_plan(name: &str, h: i64) -> String {
    format!(
        "study forward_{name} {{\n  program    {name}.dsl\n  params     {{ n = 2 }}\n  holdout    after {h}\n  budget     {{ money 0; bytes 2G; requests 0 }}\n  steps      {{ run base; report main }}\n}}\n",
        name = name,
        h = abt_gateway::plan::render_h(h),
    )
}

pub fn programs(name: &str, text: &str) -> Json {
    serde_json::json!({ format!("{}.dsl", name): text })
}

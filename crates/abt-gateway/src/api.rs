//! The JSON tool API (contracts section 5): `POST /v1/<route>` with the
//! job's Biscuit token, a JSON body, a JSON answer `{"ok": true, "result":
//! …}` or `{"ok": false, "error": {"code", "message", "detail"}}` with the
//! status 200 / 400 / 403 / 409 / 429 / 502. One route per capability of
//! PL-6.4; on every request the token is verified and scoped, time is
//! clamped, plans are guarded, bytes and requests are metered, and one row
//! goes to `pipeline.audit` (PL-7.1's table).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use abt_wire::generated::abt::wire::{Catalog, ErrorCode, QueryForm, RelationSchema};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use pipeline_wire::Identified;
use serde_json::{json, Map, Value};

use crate::horizon::Horizon;
use crate::jobs::{Jobs, Over, RunSpec, DEFAULT_BYTES_PER_JOB, DEFAULT_REQUESTS_PER_MINUTE};
use crate::layers::{LayerError, Layers};
use crate::plan::{self, Kind, Refusal};
use crate::provenance::{self, AuditRow};
use crate::query::{self, Malformed};
use crate::queue::{Queue, QueueError, DEFAULT_MAX_DEPTH};
use crate::table::{bytes_of, Table, ROW_CAP};
use crate::token::{self, Grant, TokenError, Verifier};

/// How the gateway is configured (`abt-gateway serve`).
#[derive(Clone, Debug)]
pub struct Config {
    pub max_depth: usize,
    pub bytes_per_job: u64,
    pub requests_per_minute: u32,
    /// Render every table as Markdown in the logs, for people.
    pub markdown: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_depth: DEFAULT_MAX_DEPTH,
            bytes_per_job: DEFAULT_BYTES_PER_JOB,
            requests_per_minute: DEFAULT_REQUESTS_PER_MINUTE,
            markdown: false,
        }
    }
}

/// An answer that is not a result: its status and code.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub detail: Value,
}

impl ApiError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> ApiError {
        ApiError {
            status,
            code: code.into(),
            message: message.into(),
            detail: Value::Null,
        }
    }

    fn with(mut self, detail: Value) -> ApiError {
        self.detail = detail;
        self
    }

    pub fn body(&self) -> Value {
        json!({ "ok": false, "error": { "code": self.code, "message": self.message, "detail": self.detail } })
    }
}

impl From<Malformed> for ApiError {
    fn from(m: Malformed) -> Self {
        ApiError::new(400, "malformed", m.0)
    }
}

impl From<TokenError> for ApiError {
    fn from(e: TokenError) -> Self {
        ApiError::new(403, "forbidden", e.to_string())
    }
}

impl From<Refusal> for ApiError {
    fn from(r: Refusal) -> Self {
        ApiError::new(409, r.code, r.message)
    }
}

impl From<QueueError> for ApiError {
    fn from(e: QueueError) -> Self {
        match e {
            QueueError::Busy(_) => ApiError::new(429, "busy", e.to_string()),
            QueueError::InFlight(_) => ApiError::new(429, "in_flight", e.to_string()),
        }
    }
}

impl From<Over> for ApiError {
    fn from(e: Over) -> Self {
        ApiError::new(429, "out_of_budget", e.to_string())
    }
}

impl From<LayerError> for ApiError {
    fn from(e: LayerError) -> Self {
        match &e {
            LayerError::Remote(m) => match m.code {
                ErrorCode::Busy => ApiError::new(429, "busy", e.to_string()),
                ErrorCode::OutOfBudget => ApiError::new(429, "out_of_budget", e.to_string()),
                ErrorCode::NotFound | ErrorCode::UnknownRelation | ErrorCode::UnknownVersion | ErrorCode::Malformed => ApiError::new(400, crate::layers::code_name(&m.code), e.to_string()),
                ErrorCode::Refused | ErrorCode::Unresolved | ErrorCode::Halted => ApiError::new(409, crate::layers::code_name(&m.code), e.to_string()),
                _ => ApiError::new(502, "layer", e.to_string()),
            },
            LayerError::Command { status: 1, out, err } => ApiError::new(400, "plan", err.trim().to_string()).with(json!({ "out": out })),
            LayerError::Command { .. } | LayerError::Wire(_) | LayerError::Protocol(_) => ApiError::new(502, "layer", e.to_string()),
        }
    }
}

/// The capability each route requires (PL-6.4); `None` for `health`.
pub fn capability(route: &str) -> Option<&'static str> {
    Some(match route {
        "catalog" | "schema" | "resolve_symbols" => "abt.catalog",
        "query" => "abt.query",
        "check" | "contract" | "show" => "abt.check",
        "plan_check" | "plan_run" => "abt.plan",
        "plan_report" => "abt.report",
        "query_run" => "abt.query_run",
        "submit" => "abt.submit",
        "provenance" => "provenance.write",
        _ => return None,
    })
}

/// Every route with a capability.
pub const ROUTES: [&str; 13] = [
    "catalog",
    "schema",
    "resolve_symbols",
    "query",
    "check",
    "contract",
    "show",
    "plan_check",
    "plan_run",
    "plan_report",
    "query_run",
    "submit",
    "provenance",
];

enum AuditMsg {
    Row(AuditRow),
    Flush(SyncSender<()>),
}

/// The audit's writer: rows batched to `pipeline.audit` off the request's
/// path (a row per request, PL-7.1; written within a moment, never in the
/// round trip).
struct Audit {
    tx: Mutex<Option<SyncSender<AuditMsg>>>,
    rows_total: AtomicU64,
}

impl Audit {
    fn start(layers: Arc<Layers>) -> Audit {
        let (tx, rx) = sync_channel::<AuditMsg>(4096);
        std::thread::Builder::new().name("audit".into()).spawn(move || Audit::run(layers, rx)).expect("the audit thread");
        Audit {
            tx: Mutex::new(Some(tx)),
            rows_total: AtomicU64::new(0),
        }
    }

    fn run(layers: Arc<Layers>, rx: Receiver<AuditMsg>) {
        let mut pending: Vec<AuditRow> = Vec::new();
        let mut flushes: Vec<SyncSender<()>> = Vec::new();
        loop {
            let first = match rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(m) => Some(m),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if let Some(m) = first {
                match m {
                    AuditMsg::Row(r) => pending.push(r),
                    AuditMsg::Flush(f) => flushes.push(f),
                }
                while let Ok(m) = rx.try_recv() {
                    match m {
                        AuditMsg::Row(r) => pending.push(r),
                        AuditMsg::Flush(f) => flushes.push(f),
                    }
                }
            }
            if !pending.is_empty() {
                match provenance::audit_rows(&pending).map_err(LayerError::Protocol).and_then(|b| provenance::put(&layers, b)) {
                    Ok(_) => tracing::debug!(rows = pending.len(), "audit written"),
                    Err(e) => tracing::warn!(error = %e, rows = pending.len(), "the audit could not be written"),
                }
                pending.clear();
            }
            for f in flushes.drain(..) {
                let _ = f.send(());
            }
        }
    }

    fn row(&self, mut r: AuditRow) {
        r.request = self.rows_total.fetch_add(1, Ordering::Relaxed) + 1;
        r.r = pipeline_wire::time::now();
        if let Some(tx) = self.tx.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            let _ = tx.try_send(AuditMsg::Row(r));
        }
    }

    fn flush(&self) {
        let (ftx, frx) = sync_channel(1);
        let sent = self
            .tx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|tx| tx.send(AuditMsg::Flush(ftx)).is_ok())
            .unwrap_or(false);
        if sent {
            let _ = frx.recv_timeout(std::time::Duration::from_secs(30));
        }
    }
}

/// The counters `/metrics` exports (Prometheus text).
#[derive(Default)]
pub struct Metrics {
    requests: Mutex<BTreeMap<(String, String), u64>>,
    seconds: Mutex<BTreeMap<String, (f64, u64)>>,
}

impl Metrics {
    fn record(&self, route: &str, outcome: &str, seconds: f64) {
        *self.requests.lock().unwrap_or_else(|p| p.into_inner()).entry((route.to_string(), outcome.to_string())).or_default() += 1;
        let mut s = self.seconds.lock().unwrap_or_else(|p| p.into_inner());
        let e = s.entry(route.to_string()).or_default();
        e.0 += seconds;
        e.1 += 1;
    }
}

/// The gateway: the layers behind it and the policy in front of them.
pub struct Gateway {
    pub layers: Arc<Layers>,
    verifier: Verifier,
    horizon: Horizon,
    pub jobs: Jobs,
    pub queue: Queue,
    audit: Audit,
    metrics: Metrics,
    markdown: bool,
    /// The clock, nanoseconds; replaceable by the tests.
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    /// Whether the query layer takes `holdout after TIME` (the amendment of
    /// PL-6.6): unknown until a forward plan is checked.
    after_supported: Mutex<Option<bool>>,
    next_kernel: AtomicU64,
}

/// An answer: the result and what the audit says of it.
struct Done {
    result: Value,
    subject: String,
    bytes: u64,
}

fn done_with(result: Value, subject: impl Into<String>, bytes: u64) -> Result<Done, ApiError> {
    Ok(Done {
        result,
        subject: subject.into(),
        bytes,
    })
}

fn body_str<'a>(body: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::new(400, "malformed", format!("`{}` is required", key)))
}

impl Gateway {
    pub fn new(layers: Arc<Layers>, verifier: Verifier, config: Config) -> Gateway {
        Gateway {
            queue: Queue::new(layers.kernel_count(), config.max_depth),
            audit: Audit::start(layers.clone()),
            layers,
            verifier,
            horizon: Horizon::new(),
            jobs: Jobs::new(config.bytes_per_job, config.requests_per_minute),
            metrics: Metrics::default(),
            markdown: config.markdown,
            clock: Box::new(pipeline_wire::time::now),
            after_supported: Mutex::new(None),
            next_kernel: AtomicU64::new(0),
        }
    }

    /// With a clock of the tests' choosing.
    pub fn with_clock(mut self, clock: impl Fn() -> i64 + Send + Sync + 'static) -> Gateway {
        self.clock = Box::new(clock);
        self
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    /// Waits until every audit row so far is in the data layer.
    pub fn flush_audit(&self) {
        self.audit.flush();
    }

    /// The Prometheus text of `/metrics`.
    pub fn metrics_text(&self) -> String {
        let mut s = String::new();
        s.push_str("# TYPE abt_gateway_queue_depth gauge\n");
        s.push_str(&format!("abt_gateway_queue_depth {}\n", self.queue.depth()));
        s.push_str("# TYPE abt_gateway_queue_in_flight gauge\n");
        s.push_str(&format!("abt_gateway_queue_in_flight {}\n", self.queue.in_flight()));
        s.push_str("# TYPE abt_gateway_queue_permits gauge\n");
        s.push_str(&format!("abt_gateway_queue_permits {}\n", self.queue.permits()));
        s.push_str("# TYPE abt_gateway_queue_busy_total counter\n");
        s.push_str(&format!("abt_gateway_queue_busy_total {}\n", self.queue.busy_total()));
        s.push_str("# TYPE abt_gateway_queue_served_total counter\n");
        s.push_str(&format!("abt_gateway_queue_served_total {}\n", self.queue.served_total()));
        s.push_str("# TYPE abt_gateway_jobs gauge\n");
        s.push_str(&format!("abt_gateway_jobs {}\n", self.jobs.count()));
        s.push_str("# TYPE abt_gateway_audit_rows_total counter\n");
        s.push_str(&format!("abt_gateway_audit_rows_total {}\n", self.audit.rows_total.load(Ordering::Relaxed)));
        s.push_str("# TYPE abt_gateway_requests_total counter\n");
        for ((route, outcome), n) in self.metrics.requests.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            s.push_str(&format!("abt_gateway_requests_total{{route=\"{}\",outcome=\"{}\"}} {}\n", route, outcome, n));
        }
        s.push_str("# TYPE abt_gateway_request_seconds summary\n");
        for (route, (sum, count)) in self.metrics.seconds.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            s.push_str(&format!("abt_gateway_request_seconds_sum{{route=\"{}\"}} {}\n", route, sum));
            s.push_str(&format!("abt_gateway_request_seconds_count{{route=\"{}\"}} {}\n", route, count));
        }
        s
    }

    /// One request of the tool API: the token from the header, the body as
    /// JSON; the answer's status and body. Blocking: the layers are
    /// synchronous.
    pub fn handle(&self, route: &str, authorization: Option<&str>, body: &[u8]) -> (u16, Value) {
        let started = Instant::now();
        let now = self.now();
        if route == "health" {
            let h = self.layers.health();
            let ok = h["data"] == "ok" && h["query"] == "ok" && h["kernels"].as_array().is_some_and(|k| k.iter().all(|x| x == "ok"));
            self.metrics.record(route, if ok { "ok" } else { "down" }, started.elapsed().as_secs_f64());
            return (if ok { 200 } else { 502 }, json!({ "ok": ok, "result": h }));
        }
        let Some(cap) = capability(route) else {
            self.metrics.record(route, "not_found", started.elapsed().as_secs_f64());
            return (404, ApiError::new(404, "not_found", format!("no route `{}`; the routes are {}", route, ROUTES.join(", "))).body());
        };
        let grant = match authorization
            .and_then(token::from_header)
            .ok_or_else(|| ApiError::new(403, "forbidden", "the request carries `Authorization: Biscuit <base64>`"))
            .and_then(|t| Ok(self.verifier.verify(t, now, Some(cap))?))
        {
            Ok(g) => g,
            Err(e) => {
                self.metrics.record(route, &e.code, started.elapsed().as_secs_f64());
                self.audit.row(AuditRow {
                    request: 0,
                    job: String::new(),
                    kind: route.into(),
                    subject: String::new(),
                    bytes: 0,
                    duration_ns: started.elapsed().as_nanos() as i64,
                    outcome: e.code.clone(),
                    r: now,
                });
                return (e.status, e.body());
            }
        };
        let body: Value = if body.is_empty() {
            Value::Object(Map::new())
        } else {
            match serde_json::from_slice(body) {
                Ok(v) => v,
                Err(e) => {
                    let err = ApiError::new(400, "malformed", format!("the body is not JSON: {}", e));
                    let _ = self.finish(route, &grant, started, now, Err(err.clone()));
                    return (err.status, err.body());
                }
            }
        };
        let answer = self.jobs.tick(&grant.job, now).map_err(ApiError::from).and_then(|_| self.dispatch(route, &grant, &body, now));
        match self.finish(route, &grant, started, now, answer) {
            Ok(v) => (200, json!({ "ok": true, "result": v })),
            Err(e) => (e.status, e.body()),
        }
    }

    fn finish(&self, route: &str, grant: &Grant, started: Instant, now: i64, answer: Result<Done, ApiError>) -> Result<Value, ApiError> {
        let (outcome, subject, bytes) = match &answer {
            Ok(d) => ("ok".to_string(), d.subject.clone(), d.bytes),
            Err(e) => (e.code.clone(), String::new(), 0),
        };
        let elapsed = started.elapsed();
        self.metrics.record(route, &outcome, elapsed.as_secs_f64());
        self.audit.row(AuditRow {
            request: 0,
            job: grant.job.clone(),
            kind: route.into(),
            subject,
            bytes,
            duration_ns: elapsed.as_nanos() as i64,
            outcome: outcome.clone(),
            r: now,
        });
        match &answer {
            Ok(_) => tracing::info!(job = %grant.job, route, outcome = %outcome, micros = elapsed.as_micros() as u64, "request"),
            Err(e) => tracing::info!(job = %grant.job, route, outcome = %outcome, micros = elapsed.as_micros() as u64, message = %e.message, "request"),
        }
        answer.map(|d| d.result)
    }

    fn dispatch(&self, route: &str, grant: &Grant, body: &Value, now: i64) -> Result<Done, ApiError> {
        match route {
            "catalog" => self.catalog(grant, body),
            "schema" => self.schema(body),
            "resolve_symbols" => self.resolve_symbols(body),
            "query" => self.query(grant, body),
            "check" => self.check(body),
            "contract" => self.contract(body),
            "show" => self.show(body),
            "plan_check" => self.plan_check(grant, body),
            "plan_run" => self.plan_run(grant, body),
            "plan_report" => self.plan_report(grant, body),
            "query_run" => self.query_run(grant, body),
            "submit" => self.submit(grant, body, now),
            "provenance" => self.provenance(body),
            _ => Err(ApiError::new(404, "not_found", format!("no route `{}`", route))),
        }
    }

    /// A table as the model sees it, metered against the job's bytes and
    /// logged as Markdown when asked.
    fn table(&self, grant: &Grant, batches: &[arrow::array::RecordBatch]) -> Result<(Table, u64), ApiError> {
        let bytes = bytes_of(batches);
        self.jobs.meter(&grant.job, bytes)?;
        let t = Table::of(batches, ROW_CAP);
        if self.markdown {
            tracing::info!(job = %grant.job, rows = t.len(), truncated = t.truncated, "table\n{}", t.markdown());
        }
        Ok((t, bytes))
    }

    // ------------------------------------------------------------ abt.catalog

    /// The registry's relations (D-2.4) by class: `instrument`, or the
    /// class's spec relation (`future_spec`, `option_spec`) when the
    /// predicate names its columns. A row whose class the registry does not
    /// record (a dataset ingested in process leaves it empty) passes the
    /// class filter, and the answer says so.
    fn catalog(&self, grant: &Grant, body: &Value) -> Result<Done, ApiError> {
        let class = body.get("class").and_then(Value::as_str).unwrap_or("");
        let text = body.get("predicate").and_then(Value::as_str).unwrap_or("").trim().to_string();
        let spec_cols = match class {
            "future" => &["root", "expiry", "first_trade", "last_trade", "point_value", "tick", "settlement"][..],
            "option" => &["underlying", "right", "strike", "expiry", "style", "occ", "listed"][..],
            _ => &[][..],
        };
        let uses_spec = spec_cols.iter().any(|c| text.split(|ch: char| !(ch.is_alphanumeric() || ch == '_')).any(|w| w == *c));
        let relation = if uses_spec { format!("{}_spec", class) } else { "instrument".to_string() };
        let form = QueryForm::Catalog(Box::new(Catalog {
            relation: relation.clone(),
            predicate: query::predicate(&text)?.map(Box::new),
        }));
        let (_, batches) = self.layers.query(query::request(form))?;
        let (mut t, bytes) = self.table(grant, &batches)?;
        let mut class_unknown = 0usize;
        if !uses_spec && !class.is_empty() {
            if let Some(i) = t.columns.iter().position(|c| c == "class") {
                t.rows.retain(|r| match r.get(i) {
                    Some(Value::String(c)) if c.is_empty() => {
                        class_unknown += 1;
                        true
                    }
                    Some(Value::Null) => {
                        class_unknown += 1;
                        true
                    }
                    Some(Value::String(c)) => c == class,
                    _ => false,
                });
            }
        }
        // The class's spec columns (the tick, the point value, the root,
        // the expiry: what the house sizing computes from) joined by id.
        if !uses_spec && !spec_cols.is_empty() {
            let spec = self.scan_registry(&format!("{}_spec", class), "")?;
            if let (Some(i), Some(j)) = (t.columns.iter().position(|c| c == "id"), spec.columns.iter().position(|c| c == "id")) {
                let extra: Vec<(usize, String)> = spec
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(k, c)| *k != j && !t.columns.contains(c))
                    .map(|(k, c)| (k, c.clone()))
                    .collect();
                for (_, c) in &extra {
                    t.columns.push(c.clone());
                }
                for row in t.rows.iter_mut() {
                    let id = row.get(i).cloned().unwrap_or(Value::Null);
                    let found = spec.rows.iter().find(|r| r.get(j) == Some(&id));
                    for (k, _) in &extra {
                        row.push(found.and_then(|r| r.get(*k).cloned()).unwrap_or(Value::Null));
                    }
                }
            }
        }
        let mut v = serde_json::to_value(&t).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            m.insert("relation".into(), Value::String(relation.clone()));
            if class_unknown > 0 {
                m.insert(
                    "note".into(),
                    Value::String(format!("{} row(s) carry no class in the registry and are listed under `{}`", class_unknown, class)),
                );
            }
        }
        done_with(v, relation, bytes)
    }

    /// The class's catalog as `abt data catalog` prints it (D-2.3), from
    /// the schemas the registry holds.
    fn schema(&self, body: &Value) -> Result<Done, ApiError> {
        let class = body_str(body, "class")?;
        let schemas = self.layers.schemas(None)?;
        let mine: Vec<&RelationSchema> = schemas.iter().filter(|s| s.class == class).collect();
        if mine.is_empty() {
            return Err(ApiError::new(400, "unknown_class", format!("the registry has no class `{}`", class)));
        }
        let mut text = format!("class {} {{\n", class);
        if let Some(base) = mine.iter().find_map(|s| s.calendar.clone()) {
            text.push_str(&format!("  base @{}\n\n", base));
        }
        for s in &mine {
            let args: Vec<String> = s
                .columns
                .iter()
                .filter(|c| c.role != abt_wire::generated::abt::wire::Role::ReceiptTime)
                .map(|c| match c.role {
                    abt_wire::generated::abt::wire::Role::EventTime => format!("@{}", c.name),
                    _ => format!("{}: {}", c.name, c.dimension.clone().unwrap_or_else(|| format!("{:?}", c.ty))),
                })
                .collect();
            let name = s.name.strip_prefix(&format!("{}.", class)).unwrap_or(&s.name);
            let complete = s.attributes.iter().flatten().any(|a| a.key == "complete" && a.value == "true");
            text.push_str(&format!("  {}({}){}\n", name, args.join(", "), if complete { " complete" } else { "" }));
        }
        text.push_str("}\n");
        // The registry's relations the class reads through `query` by their
        // bare names (D-2.4): the instruments, the tickers, and the class's
        // specs (a future's root, expiry, tick and point value).
        let registry: Vec<&RelationSchema> = schemas
            .iter()
            .filter(|s| s.class == "registry")
            .filter(|s| s.name == "instrument" || s.name == "ticker" || s.name == format!("{}_spec", class) || (class == "future" && s.name == "continuous"))
            .collect();
        let mut names: Vec<String> = mine.iter().map(|s| s.name.clone()).collect();
        if !registry.is_empty() {
            text.push_str("\n# the registry (query them by these names)\n");
            for s in &registry {
                let args: Vec<String> = s
                    .columns
                    .iter()
                    .filter(|c| c.role != abt_wire::generated::abt::wire::Role::ReceiptTime)
                    .map(|c| format!("{}: {:?}", c.name, c.ty))
                    .collect();
                text.push_str(&format!("{}({})\n", s.name, args.join(", ")));
                names.push(s.name.clone());
            }
        }
        done_with(json!({ "text": text, "relations": names }), class, 0)
    }

    /// Symbols to instruments through the registry's `ticker`,
    /// `instrument` and `future_spec` relations.
    fn resolve_symbols(&self, body: &Value) -> Result<Done, ApiError> {
        let symbols: Vec<String> = body
            .get("symbols")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::new(400, "malformed", "`symbols` is a list"))?
            .iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect();
        if symbols.is_empty() {
            return Err(ApiError::new(400, "malformed", "`symbols` is empty"));
        }
        let quoted: Vec<String> = symbols.iter().map(|s| s.replace(',', "")).collect();
        let tickers = self.scan_registry("ticker", &format!("symbol in ({})", quoted.join(", ")))?;
        let instruments = self.scan_registry("instrument", "")?;
        let futures = self.scan_registry("future_spec", "").unwrap_or_default();
        let by_id = |t: &Table, id: &Value| -> Option<Map<String, Value>> { t.records().into_iter().find(|r| r.get("id") == Some(id)) };
        let mut out = Vec::new();
        for r in tickers.records() {
            let id = r.get("id").cloned().unwrap_or(Value::Null);
            let inst = by_id(&instruments, &id);
            let fut = by_id(&futures, &id);
            let f = |k: &str| fut.as_ref().and_then(|f| f.get(k).cloned()).unwrap_or(Value::Null);
            let class = inst
                .as_ref()
                .and_then(|i| i.get("class").cloned())
                .filter(|c| c.as_str().is_some_and(|s| !s.is_empty()))
                .unwrap_or(if fut.is_some() { Value::String("future".into()) } else { Value::Null });
            out.push(json!({
                "symbol": r.get("symbol").cloned().unwrap_or(Value::Null),
                "abt_id": id,
                "class": class,
                "root": f("root"),
                "expiry": f("expiry"),
                "tick": f("tick"),
                "point_value": f("point_value"),
                "multiplier": inst.as_ref().and_then(|i| i.get("multiplier").cloned()).unwrap_or(Value::Null),
                "commission_per_contract": inst.as_ref().and_then(|i| i.get("commission_per_contract").cloned()).unwrap_or(Value::Null),
                "listed_from": inst.as_ref().and_then(|i| i.get("from").cloned()).or_else(|| r.get("t").cloned()).unwrap_or(Value::Null),
                "listed_to": inst.as_ref().and_then(|i| i.get("to").cloned()).or_else(|| r.get("to").cloned()).unwrap_or(Value::Null),
            }));
        }
        for s in &symbols {
            if !out.iter().any(|o| o["symbol"] == *s) {
                out.push(json!({ "symbol": s, "abt_id": Value::Null, "class": Value::Null, "root": Value::Null, "expiry": Value::Null, "tick": Value::Null, "point_value": Value::Null, "multiplier": Value::Null, "commission_per_contract": Value::Null, "listed_from": Value::Null, "listed_to": Value::Null }));
            }
        }
        done_with(Value::Array(out), "ticker", 0)
    }

    fn scan_registry(&self, relation: &str, predicate: &str) -> Result<Table, ApiError> {
        let form = QueryForm::Catalog(Box::new(Catalog {
            relation: relation.to_string(),
            predicate: query::predicate(predicate)?.map(Box::new),
        }));
        match self.layers.query(query::request(form)) {
            Ok((_, b)) => Ok(Table::of(&b, usize::MAX)),
            Err(e) if e.is_not_found() => Ok(Table::default()),
            Err(e) => Err(e.into()),
        }
    }

    // ------------------------------------------------------------ abt.query

    /// A query form, clamped to `H` (PL-7.1, Proposition PL-4.2).
    fn query(&self, grant: &Grant, body: &Value) -> Result<Done, ApiError> {
        let form = query::form(body.get("form").ok_or_else(|| ApiError::new(400, "malformed", "`form` is required"))?)?;
        let relation = query::relation_of(&form).to_string();
        let kind = query::kind_of(&form);
        if matches!(form, QueryForm::Catalog(_)) {
            return Err(ApiError::new(400, "malformed", "`catalog` is the catalog route's"));
        }
        let mut req = query::request(form);
        if let Some(r) = body.get("r_max") {
            req.receipt_filter = Some(Box::new(abt_wire::generated::abt::wire::ReceiptFilter {
                r_max: Some(query::time_of(r, "r_max")?),
            }));
        }
        req.with_seq = body.get("with_seq").and_then(Value::as_bool).unwrap_or(false);
        let seq = if kind == "chain" { None } else { self.horizon.seq_of(&self.layers, &relation, grant.h)? };
        query::clamp(&mut req, grant.h, seq);
        let effective = req.receipt_filter.as_ref().and_then(|f| f.r_max).unwrap_or(grant.h);
        let (_, batches) = self.layers.query(req)?;
        let (t, bytes) = self.table(grant, &batches)?;
        let mut v = serde_json::to_value(&t).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            m.insert("r_max".into(), Value::String(pipeline_wire::iso(effective)));
            m.insert("h".into(), Value::String(pipeline_wire::iso(grant.h)));
            m.insert("as_of_seq".into(), seq.map(Value::from).unwrap_or(Value::Null));
        }
        done_with(v, format!("{}:{}", kind, relation), bytes)
    }

    // ------------------------------------------------------------ abt.check

    fn program_body<'a>(&self, body: &'a Value) -> Result<(&'a str, String), ApiError> {
        let program = body_str(body, "program")?;
        let name = match body.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            Some(n) => n.to_string(),
            None => plan::strategy_name(program).ok_or_else(|| ApiError::new(400, "malformed", "`name` is required when the program declares no strategy"))?,
        };
        Ok((program, name))
    }

    fn check(&self, body: &Value) -> Result<Done, ApiError> {
        let (program, name) = self.program_body(body)?;
        let r = self.layers.check(self.layers.sources_for(program, &name), &name)?;
        let diagnostics: Vec<Value> = r.diagnostics.iter().flatten().map(|d| diagnostic(d)).collect();
        done_with(
            json!({
                "diagnostics": diagnostics,
                "program_hash": r.program_hash,
                "degrees_of_freedom": r.degrees_of_freedom,
                "checks": r.program_hash.is_some(),
            }),
            name,
            0,
        )
    }

    fn contract(&self, body: &Value) -> Result<Done, ApiError> {
        let (program, name) = self.program_body(body)?;
        let r = self.layers.contract(self.layers.sources_for(program, &name), &name)?;
        let text = r.text.unwrap_or_default();
        let clauses: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
        done_with(json!({ "clauses": clauses, "text": text }), name, 0)
    }

    fn show(&self, body: &Value) -> Result<Done, ApiError> {
        let (program, name) = self.program_body(body)?;
        let what = body.get("what").and_then(Value::as_str).unwrap_or("program");
        let r = self.layers.show(self.layers.sources_for(program, &name), &name, what)?;
        done_with(json!({ "text": r.text.unwrap_or_default(), "tables": [] }), format!("{}:{}", name, what), 0)
    }

    // ------------------------------------------------------------ abt.plan

    /// What `plan_check` and `plan_run` share: the plan read, the program
    /// found, the guards passed, the program checked (its hash) and in the
    /// lineage.
    fn guarded(&self, grant: &Grant, body: &Value) -> Result<Guarded, ApiError> {
        let text = body_str(body, "plan")?;
        let programs: BTreeMap<String, String> = body
            .get("programs")
            .and_then(Value::as_object)
            .ok_or_else(|| ApiError::new(400, "malformed", "`programs` maps `slug.dsl` to its text"))?
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|t| (k.clone(), t.to_string())))
            .collect();
        let study = plan::parse(text).map_err(|e| ApiError::new(400, "plan", e))?;
        let kind = plan::kind_of(&study, grant.h)?;
        plan::reveal_guard(&study, kind)?;
        plan::preset_guard(&programs)?;
        let (name, program) = plan::program_of(&study, &programs)?;
        let points = plan::grid_guard(&study, self.jobs.grid_points_left(grant))?;
        let sources = self.layers.sources_for(program, &name);
        let checked = self.layers.check(sources.clone(), &name)?;
        let Some(program_hash) = checked.program_hash.clone() else {
            return Err(ApiError::new(400, "program", format!("strategy `{}` does not check", name))
                .with(json!({ "diagnostics": checked.diagnostics.iter().flatten().map(|d| diagnostic(d)).collect::<Vec<_>>() })));
        };
        self.lineage_guard(grant, &sources, &name, &program_hash, checked.normal_form.as_deref().unwrap_or(""))?;
        Ok(Guarded {
            text: text.to_string(),
            study,
            kind,
            name,
            program: program.to_string(),
            program_hash,
            sources,
            points,
        })
    }

    /// The lineage guard (PL-6.6, PL-D102): the program is a member of the
    /// token's lineage, or the lineage is empty (the job opens one), or
    /// the program is new and would attach to the lineage by the query
    /// layer's own rule (Jaccard ≥ 0.8 of its normalised rules with a
    /// member's).
    fn lineage_guard(&self, grant: &Grant, sources: &[abt_wire::generated::abt::wire::SourceFile], name: &str, program_hash: &str, normal_form: &str) -> Result<(), ApiError> {
        if grant.lineage.is_empty() {
            return Ok(());
        }
        if let Some(l) = self.layers.lineage(sources.to_vec(), name)? {
            let mine = l.get("lineage").and_then(Value::as_str).unwrap_or("");
            if mine == grant.lineage {
                return Ok(());
            }
            return Err(Refusal {
                code: "lineage",
                message: format!(
                    "strategy `{}` ({}) belongs to lineage {}, not the job's {}",
                    name,
                    short(program_hash),
                    short(mine),
                    short(&grant.lineage)
                ),
            }
            .into());
        }
        let members = provenance::scan_all(&self.layers, "query.lineages")?;
        let mine = rules(normal_form);
        let mut best = 0.0f64;
        for m in members.records() {
            if m.get("lineage").and_then(Value::as_str) != Some(grant.lineage.as_str()) {
                continue;
            }
            let theirs: Vec<String> = m.get("rules").and_then(Value::as_str).and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default();
            best = best.max(jaccard(&mine, &theirs));
        }
        if best >= 0.8 {
            return Ok(());
        }
        Err(Refusal {
            code: "lineage",
            message: format!(
                "strategy `{}` ({}) is outside the job's lineage {}: it shares {:.0}% of its rules with the nearest member (80% attaches); revise a member of the lineage",
                name,
                short(program_hash),
                short(&grant.lineage),
                best * 100.0
            ),
        }
        .into())
    }

    /// The plan's text as the query layer takes it (`text_for_layer`).
    fn plan_text_for_layer(&self, g: &Guarded, grant: &Grant) -> Result<String, ApiError> {
        Ok(text_for_layer(&self.layers, &g.text, &g.sources, &g.name, g.kind, grant.h, &self.after_supported)?)
    }

    /// The last instant the data layer's current market versions reach.
    pub fn data_extent(&self) -> Result<Option<i64>, ApiError> {
        Ok(data_extent(&self.layers)?)
    }

    fn plan_check(&self, grant: &Grant, body: &Value) -> Result<Done, ApiError> {
        let g = self.guarded(grant, body)?;
        let text = self.plan_text_for_layer(&g, grant)?;
        let v = self.layers.plan(&["check"], &text, g.sources.clone(), &g.name)?;
        done_with(
            json!({
                "hash": v["hash"],
                "program_hash": g.program_hash,
                "normal_form": v["normal_form"],
                "trials_expected": v["trials_expected"],
                "steps": v["dag"],
                "kind": match g.kind { Kind::Development => "development", Kind::Forward => "forward" },
                "grid_points": g.points,
                "binding": v["binding"],
                "bars": v["bars"],
            }),
            v["hash"].as_str().unwrap_or("").to_string(),
            0,
        )
    }

    /// `plan run` under the job's budgets and the kernel queue; a
    /// development plan's reveal steps run once after it (PL-D103: the
    /// protocol's step 8, with no reveal route for the agent).
    fn plan_run(&self, grant: &Grant, body: &Value) -> Result<Done, ApiError> {
        let g = self.guarded(grant, body)?;
        let text = self.plan_text_for_layer(&g, grant)?;
        self.jobs.reserve_plan(grant, g.points)?;
        let permit = match self.queue.acquire(&grant.job, g.study.runs_asked().clamp(1, self.queue.permits() as u64) as usize) {
            Ok(p) => p,
            Err(e) => {
                self.jobs.release_plan(&grant.job, g.points);
                return Err(e.into());
            }
        };
        let threads = permit.n.to_string();
        let ex = match self.layers.plan(&["run", "--threads", &threads], &text, g.sources.clone(), &g.name) {
            Ok(v) => v,
            Err(e) => {
                self.jobs.release_plan(&grant.job, g.points);
                return Err(e.into());
            }
        };
        let hash = ex["hash"].as_str().unwrap_or("").to_string();
        let mut runs = runs_of(&ex);
        self.jobs.record_plan(&grant.job, &hash, &g.program_hash, run_specs(&ex, &g));
        let mut reveals = Vec::new();
        if g.kind == Kind::Development {
            for step in g.study.reveal_steps() {
                match self.layers.plan(&["reveal", &step, "--threads", &threads], &text, g.sources.clone(), &g.name) {
                    Ok(r) => {
                        let revealed = runs_of(&r);
                        self.jobs.record_plan(
                            &grant.job,
                            &hash,
                            &g.program_hash,
                            revealed.iter().filter_map(|x| x["run_id"].as_str().map(|id| (id.to_string(), None))),
                        );
                        runs.extend(revealed);
                        reveals.push(json!({ "step": step, "claims": r["claims"], "trials": r["trials"], "dsr": r["dsr"], "warnings": r["warnings"] }));
                    }
                    Err(e) => reveals.push(json!({ "step": step, "error": e.to_string() })),
                }
            }
        }
        drop(permit);
        let report = self.report_of(&hash).unwrap_or(Value::Null);
        done_with(
            json!({
                "plan_hash": hash,
                "program_hash": g.program_hash,
                "lineage": ex["lineage"],
                "runs": runs,
                "reveals": reveals,
                "trials": ex["trials"],
                "trial_kinds": ex["trial_kinds"],
                "dsr": ex["dsr"],
                "claims": ex["claims"],
                "warnings": ex["warnings"],
                "skipped": ex["skipped"],
                "embargo": ex["embargo"],
                "report": report,
            }),
            hash,
            0,
        )
    }

    /// The plan's report (Q-8) as the query layer wrote it: its
    /// `report.*` relations as tables and its text, with the outcomes and
    /// verdicts.
    pub fn report_of(&self, hash: &str) -> Result<Value, ApiError> {
        let prefix = format!("query.{}.", hash);
        let names: Vec<String> = relation_names(&self.layers)?.into_iter().filter(|n| n.starts_with(&prefix)).collect();
        let mut out = Map::new();
        for key in ["summary", "trials", "claims", "contract", "warnings", "biases", "tests"] {
            out.insert(key.into(), serde_json::to_value(Table::default()).unwrap_or(Value::Null));
        }
        let mut text = String::new();
        for n in &names {
            let suffix = &n[prefix.len()..];
            let t = provenance::scan_all(&self.layers, n)?;
            if suffix == "report.text" {
                text = t.first("text").and_then(Value::as_str).unwrap_or("").to_string();
                continue;
            }
            let key = suffix.strip_prefix("report.").unwrap_or(suffix).to_string();
            let mut capped = t;
            if capped.rows.len() > ROW_CAP {
                capped.rows.truncate(ROW_CAP);
                capped.truncated = true;
            }
            out.insert(key, serde_json::to_value(&capped).unwrap_or(Value::Null));
        }
        out.insert("text".into(), Value::String(text));
        Ok(Value::Object(out))
    }

    fn plan_report(&self, grant: &Grant, body: &Value) -> Result<Done, ApiError> {
        let hash = body_str(body, "plan_hash")?;
        if self.jobs.program_of(&grant.job, hash).is_none() {
            return Err(ApiError::new(403, "forbidden", format!("plan {} is not one this job ran", short(hash))));
        }
        let r = self.report_of(hash)?;
        done_with(r, hash.to_string(), 0)
    }

    // ------------------------------------------------------------ abt.query_run

    fn query_run(&self, grant: &Grant, body: &Value) -> Result<Done, ApiError> {
        let run = body_str(body, "run_id")?;
        let text = body_str(body, "text")?;
        if !self.jobs.owns_run(&grant.job, run) {
            return Err(ApiError::new(403, "forbidden", format!("run {} is not one this job created", short(run))));
        }
        match self.layers.query_run(run, text) {
            Ok((m, table)) => match table {
                Some((_, batches)) => {
                    let (t, bytes) = self.table(grant, &batches)?;
                    let mut v = serde_json::to_value(&t).unwrap_or(Value::Null);
                    if let (Value::Object(o), Some(txt)) = (&mut v, m.text.as_ref().filter(|s| !s.is_empty())) {
                        o.insert("text".into(), Value::String(txt.clone()));
                    }
                    done_with(v, run.to_string(), bytes)
                }
                None => done_with(json!({ "text": m.text.unwrap_or_default() }), run.to_string(), 0),
            },
            // No kernel holds the run (the query layer releases a plan's
            // runs once read back, keeping those a `ledger` step reads):
            // the gateway re-creates it on a kernel of its own pool with
            // the same π, so the same id and no new trial, and holds it
            // for the job's later questions (PL-D105).
            Err(e) if e.is_not_found() => {
                let spec = self.jobs.run_spec(&grant.job, run).ok_or_else(|| {
                    ApiError::new(
                        409,
                        "released",
                        format!(
                            "no kernel holds run {} any more and the gateway cannot re-create it (a fold's or a reveal's run); its outputs are in the data layer under kernel.{}.*",
                            short(run),
                            short(run)
                        ),
                    )
                })?;
                self.hold(grant, run, &spec)?;
                let (m, table) = self.layers.query_run(run, text)?;
                match table {
                    Some((_, batches)) => {
                        let (t, bytes) = self.table(grant, &batches)?;
                        let mut v = serde_json::to_value(&t).unwrap_or(Value::Null);
                        if let (Value::Object(o), Some(txt)) = (&mut v, m.text.as_ref().filter(|s| !s.is_empty())) {
                            o.insert("text".into(), Value::String(txt.clone()));
                        }
                        done_with(v, run.to_string(), bytes)
                    }
                    None => done_with(json!({ "text": m.text.unwrap_or_default() }), run.to_string(), 0),
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Re-creates a run on a kernel of the pool (one permit of the queue)
    /// and keeps it held for the job; the job's oldest held run beyond
    /// `HELD_PER_JOB` is released.
    fn hold(&self, grant: &Grant, run_id: &str, spec: &RunSpec) -> Result<(), ApiError> {
        let permit = self.queue.acquire(&grant.job, 1)?;
        let mut r = crate::layers::kernel_request(abt_wire::generated::abt::wire::KernelOp::Run);
        r.sources = Some(self.layers.sources_for(&spec.program, &spec.strategy));
        r.strategy = Some(spec.strategy.clone());
        r.point = Some(
            spec.point
                .iter()
                .map(|(k, v)| abt_wire::generated::abt::wire::ParamValue { name: k.clone(), value: v.clone() })
                .collect(),
        );
        r.start = spec.start;
        r.end = spec.end;
        r.horizon = spec.horizon;
        let point: BTreeMap<&str, &str> = spec.point.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let pi = json!({ "plan": spec.plan_hash, "step": spec.canonical, "point": point, "fold": spec.fold });
        r.plan_step = Some(abt_wire::to_canonical_json(&pi).map_err(|e| ApiError::new(500, "internal", e.to_string()))?);
        if !spec.mask.is_empty() {
            r.embargo_mask = Some(spec.mask.iter().map(|&(from, to)| abt_wire::generated::abt::wire::Span { from, to }).collect());
        }
        if !spec.observe.is_empty() {
            r.observe = Some(spec.observe.clone());
        }
        let (id, status) = self.layers.run_on(self.next_kernel.fetch_add(1, Ordering::Relaxed) as usize, r)?;
        drop(permit);
        if id != run_id {
            tracing::warn!(job = %grant.job, asked = %short(run_id), got = %short(&id), "a re-created run took another id; it is held under its own");
            self.jobs.record_plan(&grant.job, &spec.plan_hash, "", [(id.clone(), Some(spec.clone()))]);
        }
        tracing::info!(job = %grant.job, run = %short(&id), status = %status, "run re-created and held");
        if let Some(old) = self.jobs.hold(&grant.job, &id, HELD_PER_JOB) {
            self.layers.release(&old);
        }
        Ok(())
    }

    // ------------------------------------------------------------ abt.submit

    /// `submit` (PL-6.4, PL-P2): the hashes name a plan the job ran, the
    /// trials used are the lineage's count, the forward plan passes its
    /// guards and is checked (its hash stored), the record is written to
    /// `pipeline.submission`; one per job.
    fn submit(&self, grant: &Grant, body: &Value, now: i64) -> Result<Done, ApiError> {
        let hypothesis_id = body_str(body, "hypothesis_id")?;
        let program_hash = body_str(body, "program_hash")?;
        let plan_hash = body_str(body, "plan_hash")?;
        let program_text = body_str(body, "program_text")?;
        let plan_text = body_str(body, "plan_text")?;
        let forward_text = body_str(body, "forward_plan_text")?;
        let rationale = body.get("rationale").and_then(Value::as_str).unwrap_or("").to_string();
        let claims: Vec<String> = body
            .get("claims")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string())).collect())
            .unwrap_or_default();
        let insample = body.get("insample").cloned().unwrap_or(Value::Object(Map::new()));
        if let Some(earlier) = self.jobs.state(&grant.job).submission {
            return Err(ApiError::new(409, "submitted", format!("job {} already submitted {}", grant.job, earlier)));
        }
        match self.jobs.program_of(&grant.job, plan_hash) {
            Some(p) if p == program_hash => {}
            Some(p) => {
                return Err(ApiError::new(
                    409,
                    "hashes",
                    format!("plan {} ran program {}, not {}", short(plan_hash), short(&p), short(program_hash)),
                ))
            }
            None => return Err(ApiError::new(409, "hashes", format!("plan {} is not one this job ran", short(plan_hash)))),
        }
        // The program as submitted hashes to the hash submitted.
        let name = plan::strategy_name(program_text).ok_or_else(|| ApiError::new(400, "malformed", "`program_text` declares no strategy"))?;
        let sources = self.layers.sources_for(program_text, &name);
        let checked = self.layers.check(sources.clone(), &name)?;
        if checked.program_hash.as_deref() != Some(program_hash) {
            return Err(ApiError::new(
                409,
                "hashes",
                format!(
                    "`program_text` hashes to {}, not {}",
                    checked.program_hash.as_deref().map(short).unwrap_or("nothing"),
                    short(program_hash)
                ),
            ));
        }
        // The lineage's count (PL-6.6: the DSR uses the full count).
        let lineage = self
            .layers
            .lineage(sources.clone(), &name)?
            .ok_or_else(|| ApiError::new(409, "lineage", format!("strategy `{}` has run in no plan", name)))?;
        let lineage_id = lineage["lineage"].as_str().unwrap_or("").to_string();
        let trials_used = lineage["trials"]["count"].as_u64().unwrap_or(0);
        if !grant.lineage.is_empty() && lineage_id != grant.lineage {
            return Err(ApiError::new(409, "lineage", format!("the program belongs to lineage {}, not the job's", short(&lineage_id))));
        }
        // The forward plan: its guards, the same program, its reveal step,
        // its hash through the query layer.
        let mut programs = BTreeMap::new();
        programs.insert(format!("{}.dsl", name), program_text.to_string());
        let fwd = plan::parse(forward_text).map_err(|e| ApiError::new(400, "plan", format!("the forward plan: {}", e)))?;
        if plan::kind_of(&fwd, grant.h)? != Kind::Forward {
            return Err(Refusal {
                code: "holdout",
                message: format!("the forward plan's hold-out is `after {}`", plan::render_h(grant.h)),
            }
            .into());
        }
        plan::reveal_guard(&fwd, Kind::Forward)?;
        plan::preset_guard(&programs)?;
        let (fwd_name, _) = plan::program_of(&fwd, &programs)?;
        if fwd_name != name {
            return Err(Refusal {
                code: "lineage",
                message: format!("the forward plan runs `{}`, not the submitted `{}`", fwd_name, name),
            }
            .into());
        }
        let forward_text = plan::with_forward_reveal(forward_text, &fwd);
        let g = Guarded {
            text: forward_text.clone(),
            study: plan::parse(&forward_text).map_err(|e| ApiError::new(400, "plan", e))?,
            kind: Kind::Forward,
            name: name.clone(),
            program: program_text.to_string(),
            program_hash: program_hash.to_string(),
            sources: sources.clone(),
            points: 0,
        };
        let layer_text = self.plan_text_for_layer(&g, grant)?;
        let checked_fwd = self.layers.plan(&["check"], &layer_text, sources, &name)?;
        let forward_plan_hash = checked_fwd["hash"].as_str().unwrap_or("").to_string();
        let mut record = pipeline_wire::Submission {
            id: String::new(),
            hypothesis_id: hypothesis_id.to_string(),
            program_hash: program_hash.to_string(),
            plan_hash: plan_hash.to_string(),
            lineage: lineage_id,
            h: grant.h,
            claims: Some(claims),
            insample: insample.to_string(),
            trials_used,
            rationale,
            program_text: program_text.to_string(),
            plan_text: plan_text.to_string(),
            forward_plan_text: Some(forward_text),
            forward_plan_hash: Some(forward_plan_hash.clone()),
            agent: grant.agent.clone(),
            model: body.get("model").and_then(Value::as_str).unwrap_or("").to_string(),
            cost_usd: 0.0,
            r: now,
            job_id: Some(grant.job.clone()),
            campaign: None,
            skills_hash: None,
        };
        record.id = record.id();
        if let Err(earlier) = self.jobs.submit(&grant.job, &record.id) {
            return Err(ApiError::new(409, "submitted", format!("job {} already submitted {}", grant.job, earlier)));
        }
        provenance::put(&self.layers, provenance::submission_row(&record).map_err(|e| ApiError::new(502, "layer", e))?)?;
        self.horizon.forget(provenance::SUBMISSION);
        tracing::info!(job = %grant.job, submission = %record.id, forward_plan = %forward_plan_hash, trials_used, "submission recorded");
        done_with(
            json!({ "submission_id": record.id, "forward_plan_hash": forward_plan_hash, "trials_used": trials_used, "lineage": record.lineage, "forward_plan_text": record.forward_plan_text, "record": pipeline_wire::json::render(&pipeline_wire::Record::Submission(Box::new(record.clone()))) }),
            record.id.clone(),
            0,
        )
    }

    // ------------------------------------------------------------ provenance.write

    fn provenance(&self, body: &Value) -> Result<Done, ApiError> {
        let mut written = 0usize;
        for d in body.get("documents").and_then(Value::as_array).into_iter().flatten() {
            provenance::put(&self.layers, provenance::document_row(d).map_err(|e| ApiError::new(400, "malformed", e))?)?;
            written += 1;
        }
        for h in body.get("hypotheses").and_then(Value::as_array).into_iter().flatten() {
            provenance::put(&self.layers, provenance::hypothesis_row(h).map_err(|e| ApiError::new(400, "malformed", e))?)?;
            written += 1;
        }
        done_with(json!({ "written": written }), "pipeline.document,pipeline.hypothesis", 0)
    }
}

/// The runs a job keeps held on the gateway's kernels at once.
pub const HELD_PER_JOB: usize = 4;

/// What re-creates each run of an execution: the run steps and the grid
/// points (folds and reveals are the query layer's alone).
fn run_specs(ex: &Value, g: &Guarded) -> Vec<(String, Option<RunSpec>)> {
    let (start, end) = match &g.study.window {
        Some((a, b)) => (abt_wire::scalar::parse_timestamp(a), abt_wire::scalar::parse_timestamp(b)),
        None => (None, None),
    };
    let truncates = ex["embargo"]["truncates"].as_bool().unwrap_or(false);
    let intervals: Vec<(i64, i64)> = ex["embargo"]["intervals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| Some((i.get(0)?.as_i64()?, i.get(1)?.as_i64()?)))
        .collect();
    let horizon = if truncates { intervals.first().map(|(from, _)| from - 1) } else { None };
    let mask = if truncates { vec![] } else { intervals };
    ex["outcomes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| {
            let id = o["run_id"].as_str()?.to_string();
            if o["kind"] != "trial" || !o["fold"].is_null() {
                return Some((id, None));
            }
            let step = o["step"].as_str()?;
            let (over, observe) = g.study.run_step(step).unwrap_or((None, vec![]));
            let (s, e) = match over {
                Some((a, b)) => (abt_wire::scalar::parse_timestamp(&a), abt_wire::scalar::parse_timestamp(&b)),
                None => (start, end),
            };
            let mut point: Vec<(String, String)> = o["point"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|kv| Some((kv.get(0)?.as_str()?.to_string(), kv.get(1)?.as_str()?.to_string())))
                .collect();
            point.sort();
            Some((
                id,
                Some(RunSpec {
                    program: g.program.clone(),
                    strategy: g.name.clone(),
                    plan_hash: ex["hash"].as_str().unwrap_or("").to_string(),
                    canonical: o["canonical"].as_str().unwrap_or(step).to_string(),
                    point,
                    start: s,
                    end: e,
                    horizon,
                    mask: mask.clone(),
                    observe,
                    fold: Value::Null,
                }),
            ))
        })
        .collect()
}

/// A plan past its guards.
struct Guarded {
    text: String,
    study: plan::Study,
    kind: Kind,
    name: String,
    program: String,
    program_hash: String,
    sources: Vec<abt_wire::generated::abt::wire::SourceFile>,
    points: u64,
}

/// The runs of an execution as the route answers them.
pub fn runs_of(ex: &Value) -> Vec<Value> {
    ex["outcomes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| {
            json!({
                "step": o["step"],
                "run_id": o["run_id"],
                "status": o["status"],
                "kind": o["kind"],
                "point": o["point"],
                "decisions": o["trading"]["decisions"],
                "window": o["window"].as_array().map(|w| w.iter().map(|t| t.as_i64().map(pipeline_wire::iso).map(Value::String).unwrap_or(Value::Null)).collect::<Vec<_>>()),
                "returns": o["returns"],
                "objective": o["objective"],
            })
        })
        .collect()
}

/// A checker diagnostic's text as the route's object.
fn diagnostic(d: &str) -> Value {
    let lower = d.to_ascii_lowercase();
    let severity = if lower.contains("error") {
        "error"
    } else if lower.contains("warn") {
        "warning"
    } else {
        "note"
    };
    let code = d
        .split(|c: char| !(c.is_ascii_alphanumeric()))
        .find(|w| w.len() >= 2 && w.len() <= 4 && w.starts_with(|c: char| c.is_ascii_uppercase()) && w[1..].bytes().all(|b| b.is_ascii_digit()));
    json!({ "code": code, "severity": severity, "message": d, "span": Value::Null })
}

fn short(h: &str) -> &str {
    &h[..16.min(h.len())]
}

/// The rules of a program's normal form, each on one line, sorted: the
/// query layer's `lineage::rules`, for the attachment the guard predicts.
pub fn rules(normal_form: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in normal_form.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if cur.is_empty() && !line.contains(":-") {
            continue;
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(line);
        if cur.ends_with('.') {
            out.push(std::mem::take(&mut cur));
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.iter().filter(|x| b.contains(x)).count();
    let union = a.len() + b.len() - inter;
    inter as f64 / union as f64
}

/// The names of every relation with a current version.
pub fn relation_names(layers: &Layers) -> Result<Vec<String>, LayerError> {
    let t = Table::of(&layers.stats(None)?, usize::MAX);
    let mut names: Vec<String> = t
        .records()
        .into_iter()
        .filter(|r| r.get("current") == Some(&Value::Bool(true)))
        .filter_map(|r| r.get("relation").and_then(Value::as_str).map(str::to_string))
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// The last instant the market relations' current versions reach (the
/// data's extent: what a forward reveal may read up to).
pub fn data_extent(layers: &Layers) -> Result<Option<i64>, LayerError> {
    let t = Table::of(&layers.stats(None)?, usize::MAX);
    let mut last: Option<i64> = None;
    for r in t.records() {
        if r.get("current") != Some(&Value::Bool(true)) {
            continue;
        }
        let name = r.get("relation").and_then(Value::as_str).unwrap_or("");
        if name.starts_with("query.") || name.starts_with("kernel.") || name.starts_with("pipeline.") || name.starts_with("registry") || !name.contains('.') {
            continue;
        }
        if let Some(to) = r.get("to").map(provenance::time_of).filter(|t| *t != 0) {
            last = Some(last.map_or(to, |l| l.max(to)));
        }
    }
    Ok(last)
}

/// A plan's text as the query layer takes it: as written, or, for a
/// forward plan when the layer refuses `holdout after TIME` (the pinned
/// tree, before the amendment of PL-6.6), `holdout after H` rewritten as
/// `trailing:Ny` over the years from `H` to the data's last bar, the
/// nearest the pinned grammar can say; logged. `cache` remembers whether
/// the layer takes `after`.
pub fn text_for_layer(layers: &Layers, text: &str, sources: &[abt_wire::generated::abt::wire::SourceFile], name: &str, kind: Kind, h: i64, cache: &Mutex<Option<bool>>) -> Result<String, LayerError> {
    if kind != Kind::Forward {
        return Ok(text.to_string());
    }
    let known = *cache.lock().unwrap_or_else(|p| p.into_inner());
    if known != Some(false) {
        match layers.plan(&["check"], text, sources.to_vec(), name) {
            Ok(_) => {
                *cache.lock().unwrap_or_else(|p| p.into_inner()) = Some(true);
                return Ok(text.to_string());
            }
            Err(LayerError::Command { err, .. }) if err.contains("is not a hold-out") => {
                *cache.lock().unwrap_or_else(|p| p.into_inner()) = Some(false);
            }
            Err(e) => return Err(e),
        }
    }
    let last = data_extent(layers)?.unwrap_or(h);
    let year = 365_i64 * 86_400 * pipeline_wire::NS_PER_S + 6 * 3_600 * pipeline_wire::NS_PER_S;
    let years = ((last - h).max(0) + year - 1).div_euclid(year).clamp(1, 200) as u32;
    tracing::warn!(
        plan = name,
        h = %pipeline_wire::iso(h),
        years,
        "`holdout after H` rewritten as `trailing:{}y`: the query layer lacks the amendment of PL-6.6 (`after TIME`), so the embargo starts {} year(s) before the last bar, not at H",
        years,
        years
    );
    Ok(plan::rewrite_after_as_trailing(text, years))
}

/// Whether the query layer takes `holdout after TIME`, once known.
pub fn after_supported(cache: &Mutex<Option<bool>>) -> Option<bool> {
    *cache.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------- axum

async fn route(State(g): State<Arc<Gateway>>, Path(route): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
    let (status, body) = tokio::task::spawn_blocking(move || g.handle(&route, auth.as_deref(), &body))
        .await
        .unwrap_or_else(|e| (500, ApiError::new(500, "internal", e.to_string()).body()));
    (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(body)).into_response()
}

async fn health(State(g): State<Arc<Gateway>>) -> Response {
    let (status, body) = tokio::task::spawn_blocking(move || g.handle("health", None, b""))
        .await
        .unwrap_or_else(|e| (500, ApiError::new(500, "internal", e.to_string()).body()));
    (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), Json(body)).into_response()
}

async fn metrics(State(g): State<Arc<Gateway>>) -> Response {
    ([("content-type", "text/plain; version=0.0.4")], g.metrics_text()).into_response()
}

/// The router: `POST /v1/<route>`, `GET /v1/health`, `GET /metrics`.
pub fn router(gateway: Arc<Gateway>) -> Router {
    Router::new()
        .route("/v1/health", get(health).post(health))
        .route("/v1/{route}", post(route))
        .route("/metrics", get(metrics))
        .with_state(gateway)
}

/// Serves the tool API on `listener` until the task ends.
pub async fn serve(gateway: Arc<Gateway>, listener: tokio::net::TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router(gateway)).await
}

//! The `abt-wire` face (PL-7.1): one client to `abt-data`, a pool of kernel
//! clients and one to `abt-query`, speaking P-3's requests as the owned
//! `Message` types and receiving tables as Arrow batches. The connections
//! are Unix sockets in the deployment and in-process channels in the
//! tests; the frames are the same (A-D3, PL-7.1's decision). Every program
//! sent to a layer travels with the library (the catalog and the standard
//! library the gateway was started with, and the house presets when the
//! library lacks them), since a service reads no file of its client's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use abt_wire::generated::abt::wire::{
    ExplainResponse, KernelOp, KernelRequest, KernelResponse, Message, PutRequest, PutResponse, QueryLayerOp, QueryLayerRequest, QueryRequest, RelationSchema, SchemaRequest, SourceFile, StatsRequest,
};
use abt_wire::rpc::{Client, WireError};
use abt_wire::transport::Connection;
use abt_wire::{ErrorCode, ErrorMessage, Layer};
use arrow::array::RecordBatch;
use arrow::datatypes::SchemaRef;

/// What a layer's answer was not.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LayerError {
    /// The transport failed: the layer is down (502).
    #[error("the layer could not be reached: {0}")]
    Wire(String),
    /// The layer answered an error frame.
    #[error("{}: {}", code_name(&.0.code), .0.message.as_deref().unwrap_or(""))]
    Remote(ErrorMessage),
    /// The exchange did not go as the protocol says.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// A query-layer command ended with a non-zero status: what it said.
    #[error("{}", .err.trim())]
    Command { status: i32, out: String, err: String },
}

/// An error code's name, as `abt --json` prints it.
pub fn code_name(c: &ErrorCode) -> &'static str {
    match c {
        ErrorCode::Version => "version",
        ErrorCode::Malformed => "malformed",
        ErrorCode::UnknownRelation => "unknown_relation",
        ErrorCode::UnknownVersion => "unknown_version",
        ErrorCode::OutOfBudget => "out_of_budget",
        ErrorCode::Unresolved => "unresolved",
        ErrorCode::Refused => "refused",
        ErrorCode::Halted => "halted",
        ErrorCode::NotFound => "not_found",
        ErrorCode::Busy => "busy",
        ErrorCode::Internal => "internal",
    }
}

impl From<WireError> for LayerError {
    fn from(e: WireError) -> Self {
        match e {
            WireError::Remote(m) => LayerError::Remote(m),
            WireError::Protocol(p) => LayerError::Protocol(p),
            other => LayerError::Wire(other.to_string()),
        }
    }
}

impl LayerError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, LayerError::Remote(m) if matches!(m.code, ErrorCode::NotFound | ErrorCode::UnknownRelation))
    }
}

/// What a query-layer command printed, and its exit status.
#[derive(Clone, Debug, Default)]
pub struct Answer {
    pub out: String,
    pub err: String,
    pub status: i32,
}

impl Answer {
    /// The command's JSON (its `--json` line), when it printed one.
    pub fn json(&self) -> Result<serde_json::Value, LayerError> {
        let line = self.out.lines().find(|l| l.trim_start().starts_with('{')).unwrap_or("");
        serde_json::from_str(line).map_err(|e| LayerError::Protocol(format!("the query layer printed no JSON ({}): {}{}", e, self.out, self.err)))
    }
}

/// The three services as the gateway reaches them.
pub struct Layers {
    data: Client,
    kernels: Vec<Client>,
    query: Client,
    library: Vec<SourceFile>,
    next_kernel: AtomicUsize,
}

impl Layers {
    /// Opens the sessions (P-2.3) over the given connections.
    pub fn connect(data: Connection, kernels: Vec<Connection>, query: Connection, library: Vec<SourceFile>) -> Result<Layers, LayerError> {
        if kernels.is_empty() {
            return Err(LayerError::Wire("the kernel pool is empty: --kernels names at least one".into()));
        }
        let data = Client::connect(data, Layer::Front, "abt-gateway")?;
        let kernels = kernels.into_iter().map(|c| Client::connect(c, Layer::Front, "abt-gateway")).collect::<Result<Vec<_>, _>>()?;
        let query = Client::connect(query, Layer::Front, "abt-gateway")?;
        Ok(Layers {
            data,
            kernels,
            query,
            library: with_house_presets(library),
            next_kernel: AtomicUsize::new(0),
        })
    }

    /// Over the Unix sockets of the deployment (PL-8.4).
    pub fn sockets(data: &Path, kernels: &[PathBuf], query: &Path, library: Vec<SourceFile>) -> Result<Layers, LayerError> {
        let open = |p: &Path| Connection::connect(p).map_err(|e| LayerError::Wire(format!("{}: {}", p.display(), e)));
        let d = open(data)?;
        let ks = kernels.iter().map(|k| open(k)).collect::<Result<Vec<_>, _>>()?;
        let q = open(query)?;
        Layers::connect(d, ks, q, library)
    }

    pub fn kernel_count(&self) -> usize {
        self.kernels.len()
    }

    pub fn library(&self) -> &[SourceFile] {
        &self.library
    }

    /// The library, then the program: what a kernel or query request
    /// carries for a strategy.
    pub fn sources_for(&self, program: &str, name: &str) -> Vec<SourceFile> {
        let mut s = self.library.clone();
        s.push(SourceFile {
            path: Some(format!("{}.dsl", name)),
            text: program.to_string(),
        });
        s
    }

    // ------------------------------------------------------------ data

    pub fn schemas(&self, class: Option<&str>) -> Result<Vec<RelationSchema>, LayerError> {
        match self.data.request(Message::SchemaRequest(Box::new(SchemaRequest {
            class: class.map(str::to_string),
            relation: None,
        })))? {
            Message::SchemaResponse(r) => Ok(r.schemas.unwrap_or_default()),
            other => Err(unexpected("schema", &other)),
        }
    }

    pub fn query(&self, req: QueryRequest) -> Result<(SchemaRef, Vec<RecordBatch>), LayerError> {
        Ok(self.data.request_table(Message::QueryRequest(Box::new(req)))?)
    }

    pub fn explain(&self, mut req: QueryRequest) -> Result<ExplainResponse, LayerError> {
        req.explain = true;
        match self.data.request(Message::QueryRequest(Box::new(req)))? {
            Message::ExplainResponse(e) => Ok(*e),
            other => Err(unexpected("explain", &other)),
        }
    }

    pub fn stats(&self, relation: Option<&str>) -> Result<Vec<RecordBatch>, LayerError> {
        Ok(self
            .data
            .request_table(Message::StatsRequest(Box::new(StatsRequest {
                relation: relation.map(str::to_string),
            })))?
            .1)
    }

    /// Rows put under a relation (D-3.6): added to its current version.
    pub fn put(&self, schema: RelationSchema, batches: &[RecordBatch]) -> Result<PutResponse, LayerError> {
        let Some(first) = batches.first() else {
            return Err(LayerError::Protocol("a put carries at least one batch".into()));
        };
        let relation = schema.name.clone();
        let mut reply = self.data.call_with_table(
            Message::PutRequest(Box::new(PutRequest {
                relation,
                schema: Box::new(schema),
                attributes: None,
                cache_key: None,
                replace: false,
            })),
            &first.schema(),
            batches,
        )?;
        match reply.message()? {
            Message::PutResponse(p) => Ok(*p),
            other => Err(unexpected("put", &other)),
        }
    }

    // ------------------------------------------------------------ kernels

    fn a_kernel(&self) -> &Client {
        let i = self.next_kernel.fetch_add(1, Ordering::Relaxed) % self.kernels.len();
        &self.kernels[i]
    }

    fn kernel_request(&self, c: &Client, r: KernelRequest) -> Result<(KernelResponse, abt_wire::rpc::Reply), LayerError> {
        let mut reply = c.call(Message::KernelRequest(Box::new(r)))?;
        match reply.message()? {
            Message::KernelResponse(m) => Ok((*m, reply)),
            other => Err(unexpected("kernel", &other)),
        }
    }

    /// `check` of a strategy among `sources`: its diagnostics, hash and
    /// normal form (P-3.4).
    pub fn check(&self, sources: Vec<SourceFile>, strategy: &str) -> Result<KernelResponse, LayerError> {
        let mut r = kernel_request(KernelOp::Check);
        r.sources = Some(sources);
        r.strategy = Some(strategy.to_string());
        self.kernel_request(self.a_kernel(), r).map(|(m, _)| m)
    }

    pub fn contract(&self, sources: Vec<SourceFile>, strategy: &str) -> Result<KernelResponse, LayerError> {
        let mut r = kernel_request(KernelOp::Contract);
        r.sources = Some(sources);
        r.strategy = Some(strategy.to_string());
        self.kernel_request(self.a_kernel(), r).map(|(m, _)| m)
    }

    pub fn show(&self, sources: Vec<SourceFile>, strategy: &str, what: &str) -> Result<KernelResponse, LayerError> {
        let mut r = kernel_request(KernelOp::Show);
        r.sources = Some(sources);
        r.strategy = Some(strategy.to_string());
        r.what = Some(what.to_string());
        self.kernel_request(self.a_kernel(), r).map(|(m, _)| m)
    }

    /// A query of a run held by one of the kernels (K-7.2): asked of each
    /// until one holds it.
    #[allow(clippy::type_complexity)]
    pub fn query_run(&self, run_id: &str, text: &str) -> Result<(KernelResponse, Option<(SchemaRef, Vec<RecordBatch>)>), LayerError> {
        let mut last = LayerError::Remote(abt_wire::rpc::error(ErrorCode::NotFound, format!("no kernel holds run {}", run_id)));
        for k in &self.kernels {
            let mut r = kernel_request(KernelOp::Query);
            r.run_id = Some(run_id.to_string());
            r.text = Some(text.to_string());
            match self.kernel_request(k, r) {
                Ok((m, mut reply)) => {
                    let table = if m.has_table {
                        match reply.message()? {
                            Message::TableFollows(_) => Some(reply.table()?),
                            other => return Err(unexpected("query", &other)),
                        }
                    } else {
                        None
                    };
                    return Ok((m, table));
                }
                Err(e) if e.is_not_found() => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    /// A `run` on the `i`-th kernel to its last frame (P-3.4): the run id
    /// and its status; the kernel holds the run until `release`.
    pub fn run_on(&self, i: usize, r: KernelRequest) -> Result<(String, String), LayerError> {
        let k = &self.kernels[i % self.kernels.len()];
        let (mut m, mut reply) = self.kernel_request(k, r)?;
        let mut run_id = m.run_id.clone().unwrap_or_default();
        while m.status.as_deref() == Some("running") {
            m = match reply.message()? {
                Message::KernelResponse(next) => *next,
                other => return Err(unexpected("run", &other)),
            };
            if let Some(id) = m.run_id.as_ref().filter(|id| !id.is_empty()) {
                run_id = id.clone();
            }
        }
        Ok((run_id, m.status.unwrap_or_default()))
    }

    /// Releases a run from whichever kernel holds it.
    pub fn release(&self, run_id: &str) {
        for k in &self.kernels {
            let mut r = kernel_request(KernelOp::Control);
            r.run_id = Some(run_id.to_string());
            r.what = Some("release".into());
            if self.kernel_request(k, r).is_ok() {
                return;
            }
        }
    }

    /// Every kernel's `status` (the health route).
    pub fn kernels_status(&self) -> Vec<Result<KernelResponse, LayerError>> {
        self.kernels.iter().map(|k| self.kernel_request(k, kernel_request(KernelOp::Status)).map(|(m, _)| m)).collect()
    }

    // ------------------------------------------------------------ the query layer

    /// A command of the query service (P-3.5), its words as its command
    /// line writes them.
    pub fn command(&self, op: QueryLayerOp, args: &[&str], plan: Option<&str>, sources: Option<Vec<SourceFile>>, strategy: Option<&str>) -> Result<Answer, LayerError> {
        let r = QueryLayerRequest {
            op,
            args: Some(args.iter().map(|a| a.to_string()).collect()),
            plan: plan.map(str::to_string),
            sources,
            strategy: strategy.map(str::to_string),
        };
        let mut reply = self.query.call(Message::QueryLayerRequest(Box::new(r)))?;
        let mut a = Answer::default();
        loop {
            match reply.message()? {
                Message::QueryLayerResponse(r) => {
                    a.out.push_str(r.out.as_deref().unwrap_or(""));
                    a.err.push_str(r.err.as_deref().unwrap_or(""));
                    if r.done {
                        a.status = r.status;
                        return Ok(a);
                    }
                }
                other => return Err(unexpected("query layer", &other)),
            }
        }
    }

    /// `abt plan <words> --json` over a plan's text and its program; the
    /// command's JSON, or what it complained of.
    pub fn plan(&self, words: &[&str], plan: &str, sources: Vec<SourceFile>, strategy: &str) -> Result<serde_json::Value, LayerError> {
        let mut args: Vec<&str> = words.to_vec();
        args.push("--json");
        let a = self.command(QueryLayerOp::Plan, &args, Some(plan), Some(sources), Some(strategy))?;
        if a.status != 0 {
            return Err(LayerError::Command {
                status: a.status,
                out: a.out,
                err: a.err,
            });
        }
        a.json()
    }

    /// `abt lineage <strategy> --json`: `None` when no plan has run it.
    pub fn lineage(&self, sources: Vec<SourceFile>, strategy: &str) -> Result<Option<serde_json::Value>, LayerError> {
        let a = self.command(QueryLayerOp::Lineage, &[strategy, "--json"], None, Some(sources), Some(strategy))?;
        if a.status != 0 {
            if a.err.contains("is in no lineage") {
                return Ok(None);
            }
            return Err(LayerError::Command {
                status: a.status,
                out: a.out,
                err: a.err,
            });
        }
        a.json().map(Some)
    }

    /// Whether each service answers: data (its schemas), every kernel (its
    /// status), the query layer (a command's usage).
    pub fn health(&self) -> serde_json::Value {
        let data = match self.schemas(None) {
            Ok(_) => "ok".to_string(),
            Err(e) => e.to_string(),
        };
        let kernels: Vec<String> = self.kernels_status().into_iter().map(|r| r.map(|_| "ok".to_string()).unwrap_or_else(|e| e.to_string())).collect();
        let query = match self.command(QueryLayerOp::Metrics, &[], None, None, None) {
            Ok(_) => "ok".to_string(),
            Err(e) => e.to_string(),
        };
        serde_json::json!({ "data": data, "kernels": kernels, "query": query })
    }
}

fn unexpected(what: &str, m: &Message) -> LayerError {
    LayerError::Protocol(format!("a {} request answered with {:?}", what, std::mem::discriminant(m)))
}

/// An empty kernel request (P-3.4; ERRATA E-54: a field a request does not
/// take is left out).
pub fn kernel_request(op: KernelOp) -> KernelRequest {
    KernelRequest {
        op,
        sources: None,
        strategy: None,
        point: None,
        what: None,
        run_id: None,
        text: None,
        to_seq: None,
        start: None,
        end: None,
        memory_budget: None,
        pushdown: true,
        progress_every: None,
        checkpoint_every: None,
        from_checkpoint: None,
        horizon: None,
        plan_step: None,
        embargo_mask: None,
        observe: None,
        mode: None,
        session: None,
        deployment: None,
    }
}

/// Every `.dsl` file under the directories, in path order: the catalog and
/// the standard library (`--library DIR`, repeatable).
pub fn library_from_dirs(dirs: &[PathBuf]) -> Result<Vec<SourceFile>, String> {
    fn collect(p: &Path, out: &mut Vec<PathBuf>) {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    collect(&e.path(), out);
                }
            }
        } else if p.extension().is_some_and(|x| x == "dsl") {
            out.push(p.to_path_buf());
        }
    }
    let mut files = Vec::new();
    for d in dirs {
        if !d.exists() {
            return Err(format!("{}: no such file or directory", d.display()));
        }
        collect(d, &mut files);
    }
    files.sort();
    files.dedup();
    files
        .into_iter()
        .map(|p| {
            Ok(SourceFile {
                text: std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", p.display(), e))?,
                path: Some(p.to_string_lossy().into_owned()),
            })
        })
        .collect()
}

/// The library with the house presets added when it defines neither.
fn with_house_presets(mut library: Vec<SourceFile>) -> Vec<SourceFile> {
    let defines = |name: &str| {
        library.iter().any(|s| {
            s.text
                .lines()
                .any(|l| l.trim_start().starts_with(&format!("execution {} ", name)) || l.trim_start().starts_with(&format!("execution {}{{", name)))
        })
    };
    if !defines("futures_house") && !defines("equities_house") {
        library.push(SourceFile {
            path: Some("house_presets.dsl".into()),
            text: crate::HOUSE_PRESETS.to_string(),
        });
    }
    library
}

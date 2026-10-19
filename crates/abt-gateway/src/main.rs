//! `abt-gateway` (PL-7.1, PL-7.3): `serve` (the tool API over the three
//! services), `forward` (the scheduler), `retire`, `audit tail` and `token
//! keygen|mint`; structured JSON logs, `--json` on every command.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use serde_json::json;

use abt_gateway::api::{self, Gateway};
use abt_gateway::forward;
use abt_gateway::layers::{library_from_dirs, Layers};
use abt_gateway::provenance;
use abt_gateway::table::Table;
use abt_gateway::token::{mint, Grant, Verifier};

#[derive(Parser)]
#[command(name = "abt-gateway", about = "The only entry to the three backtester services for the pipeline (PL-7)")]
struct Cli {
    /// Print the command's answer as JSON.
    #[arg(long, global = true)]
    json: bool,
    /// Logs as text rather than JSON lines.
    #[arg(long, global = true)]
    log_text: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

/// How the layers are reached (PL-8.4: Unix sockets on the machine).
#[derive(Args, Clone, Debug)]
struct LayerArgs {
    /// The data service's socket (`data.sock` under --store-sockets-dir).
    #[arg(long)]
    data: Option<PathBuf>,
    /// The kernel services' sockets, comma-separated (`kernel-1.sock`, …
    /// under --store-sockets-dir; four by default).
    #[arg(long, value_delimiter = ',')]
    kernels: Vec<PathBuf>,
    /// The query service's socket (`query.sock` under --store-sockets-dir).
    #[arg(long)]
    query: Option<PathBuf>,
    /// The directory the services' sockets live in; relative socket paths
    /// are under it.
    #[arg(long)]
    store_sockets_dir: Option<PathBuf>,
    /// A directory of `.dsl` sources sent with every program: the
    /// backtester's `corpus-v2/catalog` and `corpus-v2/lib` (repeatable).
    #[arg(long)]
    library: Vec<PathBuf>,
}

impl LayerArgs {
    fn connect(&self) -> Result<Arc<Layers>, String> {
        let under = |p: PathBuf| match &self.store_sockets_dir {
            Some(d) if p.is_relative() => d.join(p),
            _ => p,
        };
        let data = under(self.data.clone().unwrap_or_else(|| PathBuf::from("data.sock")));
        let query = under(self.query.clone().unwrap_or_else(|| PathBuf::from("query.sock")));
        let kernels: Vec<PathBuf> = if self.kernels.is_empty() {
            if self.store_sockets_dir.is_none() {
                return Err("--kernels S1,S2,… names the kernel services' sockets (or --store-sockets-dir DIR holds kernel-1.sock … kernel-4.sock)".into());
            }
            (1..=4).map(|i| under(PathBuf::from(format!("kernel-{}.sock", i)))).filter(|p| p.exists()).collect()
        } else {
            self.kernels.iter().cloned().map(under).collect()
        };
        let library = library_from_dirs(&self.library)?;
        if library.is_empty() {
            tracing::warn!("no --library DIR: programs travel without the catalog and the standard library; only self-contained programs will check");
        }
        Layers::sockets(&data, &kernels, &query, library).map(Arc::new).map_err(|e| e.to_string())
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// The JSON tool API on a loopback port over the three services.
    Serve {
        #[command(flatten)]
        layers: LayerArgs,
        #[arg(long, default_value = "127.0.0.1:7777")]
        listen: String,
        /// The orchestrator's public key (PEM, 64 hex digits, or raw).
        #[arg(long)]
        token_public_key: PathBuf,
        /// The kernel queue's depth past which requests are `busy`.
        #[arg(long, default_value_t = abt_gateway::queue::DEFAULT_MAX_DEPTH)]
        max_depth: usize,
        /// Bytes a job may read through `query`.
        #[arg(long, default_value_t = abt_gateway::jobs::DEFAULT_BYTES_PER_JOB)]
        bytes_per_job: u64,
        /// Requests a job may make per minute.
        #[arg(long, default_value_t = abt_gateway::jobs::DEFAULT_REQUESTS_PER_MINUTE)]
        requests_per_minute: u32,
        /// Log every table as Markdown, for people.
        #[arg(long)]
        markdown: bool,
    },
    /// The forward scheduler: reveals due submissions every period.
    Forward {
        #[command(flatten)]
        layers: LayerArgs,
        /// How often to look (`week`, `1d`, `30m`).
        #[arg(long, default_value = "week")]
        every: String,
        /// The report period a reveal waits for (defaults to --every).
        #[arg(long)]
        period: Option<String>,
        /// One pass, then exit.
        #[arg(long)]
        once: bool,
        /// Where outcomes go as JSON when PIPELINE_BROKERS is unset.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Retires a submission (its record stays; its status says why).
    Retire {
        #[command(flatten)]
        layers: LayerArgs,
        /// The submission's id (or its first hex digits).
        id: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// The audit (`pipeline.audit`).
    Audit {
        #[command(flatten)]
        layers: LayerArgs,
        #[command(subcommand)]
        what: AuditCmd,
    },
    /// Tokens, for tests and operators (the orchestrator mints the jobs').
    Token {
        #[command(subcommand)]
        what: TokenCmd,
    },
}

#[derive(Subcommand)]
enum AuditCmd {
    /// The last rows of the audit.
    Tail {
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// One job's rows only.
        #[arg(long)]
        job: Option<String>,
    },
}

#[derive(Subcommand)]
enum TokenCmd {
    /// A fresh ed25519 root key pair: the private key to FILE, the public
    /// key to FILE.pub (hex, what --token-public-key reads).
    Keygen { file: PathBuf },
    /// A token signed by the private key at FILE.
    Mint {
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        job: String,
        #[arg(long, default_value = "quant")]
        agent: String,
        /// Capabilities, comma-separated.
        #[arg(long, value_delimiter = ',', default_value = "abt.catalog,abt.query,abt.check,abt.plan,abt.report,abt.query_run,abt.submit")]
        caps: Vec<String>,
        /// The horizon, ISO-8601.
        #[arg(long)]
        h: String,
        #[arg(long, default_value = "")]
        lineage: String,
        #[arg(long, default_value_t = 4)]
        plan_runs: u32,
        #[arg(long, default_value_t = 25)]
        grid_points: u32,
        /// Validity, a duration from now.
        #[arg(long, default_value = "1h")]
        ttl: String,
    },
}

fn init_logs(text: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    if text {
        tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();
    } else {
        tracing_subscriber::fmt().json().with_env_filter(filter).with_writer(std::io::stderr).init();
    }
}

fn main() {
    let cli = Cli::parse();
    init_logs(cli.log_text);
    let code = match run(cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e);
            2
        }
    };
    std::process::exit(code);
}

fn brokers() -> Option<String> {
    std::env::var("PIPELINE_BROKERS").ok().filter(|b| !b.is_empty())
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.cmd {
        Cmd::Serve {
            layers,
            listen,
            token_public_key,
            max_depth,
            bytes_per_job,
            requests_per_minute,
            markdown,
        } => {
            let verifier = Verifier::from_file(&token_public_key)?;
            let layers = layers.connect()?;
            let gateway = Arc::new(Gateway::new(
                layers,
                verifier,
                api::Config {
                    max_depth,
                    bytes_per_job,
                    requests_per_minute,
                    markdown,
                },
            ));
            let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind(&listen).await.map_err(|e| format!("{}: {}", listen, e))?;
                tracing::info!(listen = %listen, kernels = gateway.queue.permits(), "serving the tool API");
                api::serve(gateway, listener).await.map_err(|e| e.to_string())
            })
        }
        Cmd::Forward { layers, every, period, once, out } => {
            let every_ns = forward::parse_duration(&every)?;
            let cfg = forward::Config {
                period: period.as_deref().map(forward::parse_duration).transpose()?.unwrap_or(every_ns),
                out,
                brokers: brokers(),
            };
            let layers = layers.connect()?;
            loop {
                let revealed = forward::tick(&layers, &cfg, pipeline_wire::time::now())?;
                if cli.json {
                    println!("{}", serde_json::to_string(&json!({ "revealed": revealed })).unwrap_or_default());
                } else {
                    for r in &revealed {
                        println!(
                            "{} reveal {} {} .. {} {}",
                            r.submission_id,
                            r.reveal,
                            pipeline_wire::iso(r.forward_from),
                            pipeline_wire::iso(r.forward_to),
                            r.status
                        );
                    }
                    if revealed.is_empty() {
                        println!("nothing due");
                    }
                }
                if once {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_nanos(every_ns.max(1) as u64));
            }
        }
        Cmd::Retire { layers, id, reason, out } => {
            let cfg = forward::Config { period: 0, out, brokers: brokers() };
            let layers = layers.connect()?;
            let r = forward::retire(&layers, &cfg, &id, &reason, pipeline_wire::time::now())?;
            if cli.json {
                println!("{}", serde_json::to_string(&r).unwrap_or_default());
            } else {
                println!("retired {}: {}", r.submission_id, reason);
            }
            Ok(())
        }
        Cmd::Audit { layers, what } => {
            let layers = layers.connect()?;
            let AuditCmd::Tail { lines, job } = what;
            let mut t = provenance::scan_all(&layers, provenance::AUDIT).map_err(|e| e.to_string())?;
            if let Some(j) = &job {
                let i = t.columns.iter().position(|c| c == "job");
                t.rows.retain(|r| i.is_some_and(|i| r.get(i).and_then(|v| v.as_str()) == Some(j.as_str())));
            }
            let keep = t.rows.len().saturating_sub(lines);
            t.rows.drain(..keep);
            if cli.json {
                println!("{}", serde_json::to_string(&t).unwrap_or_default());
            } else {
                print!(
                    "{}",
                    Table {
                        columns: t.columns,
                        rows: t.rows,
                        truncated: false
                    }
                    .markdown()
                );
            }
            Ok(())
        }
        Cmd::Token { what } => match what {
            TokenCmd::Keygen { file } => {
                let kp = biscuit_auth::KeyPair::new();
                let private: String = kp.private().to_bytes().iter().map(|b| format!("{:02x}", b)).collect();
                let public: String = kp.public().to_bytes().iter().map(|b| format!("{:02x}", b)).collect();
                std::fs::write(&file, &private).map_err(|e| format!("{}: {}", file.display(), e))?;
                let pub_path = file.with_extension("pub");
                std::fs::write(&pub_path, &public).map_err(|e| format!("{}: {}", pub_path.display(), e))?;
                if cli.json {
                    println!("{}", json!({ "private_key": file, "public_key": pub_path }));
                } else {
                    println!("private key {}\npublic key  {}", file.display(), pub_path.display());
                }
                Ok(())
            }
            TokenCmd::Mint {
                key,
                job,
                agent,
                caps,
                h,
                lineage,
                plan_runs,
                grid_points,
                ttl,
            } => {
                let hex = std::fs::read_to_string(&key).map_err(|e| format!("{}: {}", key.display(), e))?;
                let hex = hex.trim();
                let raw: Vec<u8> = (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|e| e.to_string()))
                    .collect::<Result<_, _>>()?;
                let private = biscuit_auth::PrivateKey::from_bytes(&raw, biscuit_auth::Algorithm::Ed25519).map_err(|e| e.to_string())?;
                let root = biscuit_auth::KeyPair::from(&private);
                let now = pipeline_wire::time::now();
                let grant = Grant {
                    job,
                    agent,
                    caps: caps.into_iter().collect(),
                    h: pipeline_wire::parse_iso(&h)?,
                    lineage,
                    plan_runs,
                    grid_points,
                    expires: now + forward::parse_duration(&ttl)?,
                };
                let t = mint(&root, &grant)?;
                if cli.json {
                    println!("{}", json!({ "token": t, "expires": pipeline_wire::iso(grant.expires) }));
                } else {
                    println!("{}", t);
                }
                Ok(())
            }
        },
    }
}

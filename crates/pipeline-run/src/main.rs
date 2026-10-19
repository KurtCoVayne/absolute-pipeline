//! `pipeline-run`: the MVP as one process (the owner's cut of Oct 19, 2026;
//! the debts are listed in docs/iterations/01-mvp.md).
//!
//! - `fetch`: the seed posts by id through xAI's `x_search` (PL-D9: no X API
//!   bearer token exists), every response's cost into the X guard (a hard
//!   cap), each text checked against the public mirror's copy (PL-D7).
//! - `run`: triage → analyst → quant, each job a `python -m agents_harness`
//!   subprocess over a job directory (contracts section 3), its token minted
//!   by `abt-gateway token mint`, its records appended to JSON-lines files
//!   (the file-backed queue), the model spend summed into the OpenAI guard.
//! - `page`: one static HTML page of the posts and their submissions.
//!
//! Nothing here trades and nothing writes to X.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use pipeline_wire::{id_of, iso, snowflake_created, NS_PER_S};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "pipeline-run")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// The seed posts by id through xAI, checked against the mirror's copies.
    Fetch {
        #[arg(long)]
        posts: PathBuf,
        #[arg(long)]
        mirror: PathBuf,
        #[arg(long)]
        queue: PathBuf,
        #[arg(long)]
        xai_key: PathBuf,
        /// The X guard for this command, dollars (a hard cap).
        #[arg(long, default_value_t = 24.0)]
        cap_usd: f64,
        #[arg(long, default_value_t = 10)]
        batch: usize,
        #[arg(long, default_value = "grok-4.20-non-reasoning")]
        model: String,
    },
    /// Triage, analyst and quant over the queue's documents.
    Run {
        #[arg(long)]
        queue: PathBuf,
        #[arg(long)]
        skills: PathBuf,
        #[arg(long)]
        python: PathBuf,
        #[arg(long)]
        harness_dir: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:7777")]
        gateway: String,
        #[arg(long)]
        gateway_bin: PathBuf,
        #[arg(long)]
        signing_key: PathBuf,
        #[arg(long)]
        openai_key: PathBuf,
        #[arg(long, default_value_t = 12)]
        parallel: usize,
        /// The OpenAI guard, dollars (a hard cap: no job starts past it).
        #[arg(long, default_value_t = 50.0)]
        cap_usd: f64,
        #[arg(long, default_value = "seed-1")]
        campaign: String,
        /// Only these post ids (comma-separated); all when empty.
        #[arg(long, default_value = "")]
        only: String,
        /// Which triage verdicts go on to the analyst: `candidate` (the
        /// feed's relevance gate, PL-5.5) or `none` (every post: a curated
        /// campaign, PL-D12).
        #[arg(long, default_value = "candidate")]
        gate: String,
    },
    /// The static page.
    Page {
        #[arg(long)]
        queue: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let r = match cli.cmd {
        Cmd::Fetch {
            posts,
            mirror,
            queue,
            xai_key,
            cap_usd,
            batch,
            model,
        } => fetch(&posts, &mirror, &queue, &xai_key, cap_usd, batch, &model),
        Cmd::Run {
            queue,
            skills,
            python,
            harness_dir,
            gateway,
            gateway_bin,
            signing_key,
            openai_key,
            parallel,
            cap_usd,
            campaign,
            only,
            gate,
        } => run(RunCfg {
            queue,
            skills,
            python,
            harness_dir,
            gateway,
            gateway_bin,
            signing_key,
            openai_key: fs::read_to_string(&openai_key).map(|s| s.trim().to_string()).unwrap_or_default(),
            parallel,
            cap_usd,
            campaign,
            only: only.split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
            gate,
        }),
        Cmd::Page { queue, out } => page(&queue, &out),
    };
    if let Err(e) = r {
        eprintln!("pipeline-run: {}", e);
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------- the file-backed queue

/// One writer at a time on the queue's files.
static APPEND: Mutex<()> = Mutex::new(());

/// Appends one JSON record as a line, in one write under the queue's lock,
/// synced (the queue's durability): concurrent jobs never interleave a line.
fn append(queue: &Path, topic: &str, v: &Value) -> Result<(), String> {
    let p = queue.join(format!("{}.jsonl", topic));
    let line = format!("{}\n", v);
    let _g = APPEND.lock().unwrap_or_else(|e| e.into_inner());
    let mut f = OpenOptions::new().create(true).append(true).open(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    f.sync_data().map_err(|e| e.to_string())
}

fn read_topic(queue: &Path, topic: &str) -> Vec<Value> {
    fs::read_to_string(queue.join(format!("{}.jsonl", topic)))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn now_ns() -> i64 {
    pipeline_wire::time::now()
}

fn hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

// ---------------------------------------------------------------- fetch

struct Seed {
    created: String,
    handle: String,
    id: String,
    url: String,
}

fn seeds(path: &Path) -> Result<Vec<Seed>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    Ok(text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("created_utc") && !l.trim().is_empty())
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() >= 4).then(|| Seed {
                created: c[0].into(),
                handle: c[1].into(),
                id: c[2].into(),
                url: c[3].into(),
            })
        })
        .collect())
}

fn squash(s: &str) -> String {
    s.split_whitespace().filter(|w| !w.starts_with("http")).collect::<Vec<_>>().join(" ")
}

fn fetch(posts: &Path, mirror: &Path, queue: &Path, key: &Path, cap_usd: f64, batch: usize, model: &str) -> Result<(), String> {
    fs::create_dir_all(queue.join("raw")).map_err(|e| e.to_string())?;
    if queue.join("docs.jsonl").exists() {
        return Err(format!("{} holds documents already; the fetch runs once", queue.display()));
    }
    let key = fs::read_to_string(key).map_err(|e| e.to_string())?.trim().to_string();
    let rows = seeds(posts)?;
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(600)).build().map_err(|e| e.to_string())?;
    let mut spent = 0.0f64;
    let mut got: BTreeMap<String, (Value, String)> = BTreeMap::new(); // id -> (post, raw id)
    for (k, chunk) in rows.chunks(batch.max(1)).enumerate() {
        // The guard: a request is made only while the spend plus the
        // dearest response seen so far stays under the cap.
        if spent + 1.0 > cap_usd {
            eprintln!("X guard: ${:.2} spent of ${:.2}; no further request", spent, cap_usd);
            break;
        }
        let mut handles: Vec<String> = chunk.iter().map(|s| s.handle.clone()).collect();
        handles.sort();
        handles.dedup();
        let ids: Vec<String> = chunk.iter().map(|s| format!("{} (by @{}, {})", s.id, s.handle, s.url)).collect();
        let prompt = format!(
            "Fetch each of these X posts by its status id with the thread fetch tool (post_id), and output one JSON object per line \
             with exactly the keys id (the status id as a string), url, created_at (ISO-8601 UTC), text (the post's text verbatim, \
             no paraphrase, no summary), urls (the expanded links in the post), quoted_id (or null), reply_to_id (or null). \
             Output JSON lines only, nothing else. Omit a post you cannot fetch verbatim.\n{}",
            ids.join("\n")
        );
        let body = json!({
            "model": model,
            "input": prompt,
            "tools": [{ "type": "x_search", "allowed_x_handles": handles }],
            "max_tool_calls": 2 * chunk.len() + 4,
        });
        let t0 = Instant::now();
        let resp = client.post("https://api.x.ai/v1/responses").bearer_auth(&key).json(&body).send().map_err(|e| e.to_string())?;
        let status = resp.status();
        let bytes = resp.bytes().map_err(|e| e.to_string())?;
        let raw_path = queue.join("raw").join(format!("xai-{:02}.json", k));
        fs::write(&raw_path, &bytes).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let ticks = v["usage"]["cost_in_usd_ticks"].as_f64().unwrap_or(0.0);
        let cost = ticks * 1e-10;
        spent += cost;
        append(
            queue,
            "spend",
            &json!({"service": "xai", "request": k, "status": status.as_u16(), "cost_usd": cost, "spent_usd": spent, "x_posts_fetched": v["usage"]["server_side_tool_usage_details"]["x_posts_fetched"], "wall_s": t0.elapsed().as_secs_f64(), "r": iso(now_ns())}),
        )?;
        eprintln!(
            "xai request {} ({} ids): HTTP {}, ${:.3}, {:.0} s, spent ${:.2}",
            k,
            chunk.len(),
            status,
            cost,
            t0.elapsed().as_secs_f64(),
            spent
        );
        let raw_id = hex(&bytes);
        for o in v["output"].as_array().into_iter().flatten() {
            if o["type"] != "message" {
                continue;
            }
            for c in o["content"].as_array().into_iter().flatten() {
                for line in c["text"].as_str().unwrap_or("").lines() {
                    let line = line.trim().trim_start_matches("```json").trim_start_matches("```").trim();
                    if let Ok(p) = serde_json::from_str::<Value>(line) {
                        if let Some(id) = p["id"].as_str().map(String::from).or_else(|| p["id"].as_u64().map(|n| n.to_string())) {
                            got.insert(id, (p, raw_id.clone()));
                        }
                    }
                }
            }
        }
    }
    let r = now_ns();
    let mut n_docs = 0;
    for s in &rows {
        let post_id: u64 = s.id.parse().map_err(|_| format!("{} is not a post id", s.id))?;
        let fixed_at = snowflake_created(post_id);
        let m_path = mirror.join(format!("{}.json", s.id));
        let mirror_rec: Option<Value> = fs::read(&m_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let m_text = mirror_rec.as_ref().and_then(|m| m["tweet"]["text"].as_str()).map(String::from);
        let x = got.get(&s.id);
        let (text, via, transcription, verified, raw_id) = match (x, &m_text) {
            (Some((p, rid)), Some(mt)) => {
                let xt = p["text"].as_str().unwrap_or("");
                let same = squash(xt) == squash(mt);
                (mt.clone(), "xai+mirror", if same { 0.0 } else { 1.0 }, same, rid.clone())
            }
            (Some((p, rid)), None) => (p["text"].as_str().unwrap_or("").to_string(), "xai", 0.0, false, rid.clone()),
            (None, Some(mt)) => (mt.clone(), "mirror", 0.0, true, hex(&fs::read(&m_path).unwrap_or_default())),
            (None, None) => {
                append(
                    queue,
                    "failures",
                    &json!({"post_id": s.id, "handle": s.handle, "stage": "fetch", "reason": "not_fetched", "detail": "neither xAI nor the mirror returned the post", "r": iso(r)}),
                )?;
                continue;
            }
        };
        let tweet = mirror_rec.as_ref().map(|m| m["tweet"].clone()).unwrap_or(Value::Null);
        let author_id = tweet["author"]["id"].as_str().unwrap_or("").to_string();
        let mut urls: Vec<String> = x
            .and_then(|(p, _)| p["urls"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|u| u.as_str().map(String::from))
            .collect();
        for f in tweet["raw_text"]["facets"].as_array().into_iter().flatten() {
            if f["type"] == "url" {
                if let Some(u) = f["replacement"].as_str() {
                    urls.push(u.to_string());
                }
            }
        }
        urls.sort();
        urls.dedup();
        let cashtags: Vec<String> = tweet["raw_text"]["facets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| f["type"] == "symbol")
            .filter_map(|f| f["original"].as_str().map(String::from))
            .collect();
        let quoted = x
            .and_then(|(p, _)| p["quoted_id"].as_str().map(String::from))
            .or_else(|| tweet["quote"]["id"].as_str().map(String::from));
        let reply = x
            .and_then(|(p, _)| p["reply_to_id"].as_str().map(String::from))
            .or_else(|| tweet["replying_to_status"].as_str().map(String::from));
        let mut parents = vec![];
        if let Some(q) = &quoted {
            parents.push(json!({"kind": "Quote", "source_id": q}));
        }
        if let Some(q) = &reply {
            parents.push(json!({"kind": "Reply", "source_id": q}));
        }
        // The id over the logical fields (PL-4.1), as pipeline-wire's Document.
        let id = id_of(&json!({
            "source": "X", "author": {"id": author_id, "handle": s.handle},
            "source_id": [s.id.clone()], "p": fixed_at, "fixed_at": fixed_at, "r": r, "raw_ids": [raw_id.clone()],
        }))
        .hex();
        let doc = json!({
            "record": "Document", "id": id, "version": 1, "source": "X", "source_id": s.id,
            "author": {"id": author_id, "handle": s.handle},
            "p": iso(fixed_at), "fixed_at": iso(fixed_at), "r": iso(r), "r_n": iso(r),
            "raw_ids": [raw_id], "parents": parents, "text": text,
            "entities": {"cashtags": cashtags, "urls": urls.clone()},
            "links": urls.iter().map(|u| json!({"url": u, "state": "Skipped", "reason": "fetching links is cut today (debt)"})).collect::<Vec<_>>(),
            "attachments": [{"kind": "permalink", "url": s.url}],
            "lang": tweet["lang"].as_str().unwrap_or("en"),
            "flags": {"injection": 0.0, "spam": 0.0, "transcription": transcription, "verified": verified},
            "via": via, "seed_created_utc": s.created,
        });
        append(queue, "docs", &doc)?;
        n_docs += 1;
    }
    eprintln!("{} documents of {} seed posts; xAI returned {}; X spend ${:.2}", n_docs, rows.len(), got.len(), spent);
    Ok(())
}

// ---------------------------------------------------------------- run

struct RunCfg {
    queue: PathBuf,
    skills: PathBuf,
    python: PathBuf,
    harness_dir: PathBuf,
    gateway: String,
    gateway_bin: PathBuf,
    signing_key: PathBuf,
    openai_key: String,
    parallel: usize,
    cap_usd: f64,
    campaign: String,
    only: Vec<String>,
    gate: String,
}

#[derive(Clone)]
struct Def {
    name: String,
    stage: String,
    model: String,
    cutoff: i64,
    skills: Vec<String>,
    caps: Vec<String>,
    budget: Value,
    toml_bytes: Vec<u8>,
    hash: String,
    skills_hash: String,
}

fn load_def(skills_dir: &Path, name: &str) -> Result<Def, String> {
    let path = skills_dir.join("definitions").join(format!("{}.toml", name));
    let bytes = fs::read(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let t: toml::Value = toml::from_str(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {}", path.display(), e))?;
    let a = &t["agent"];
    let skills: Vec<String> = a["skills"].as_array().cloned().unwrap_or_default().iter().filter_map(|s| s.as_str().map(String::from)).collect();
    let mut pairs = vec![];
    for s in &skills {
        let p = skills_dir.join("skills").join(s).join("SKILL.md");
        let b = fs::read(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
        pairs.push(json!([s, hex(&b)]));
    }
    pairs.sort_by(|x, y| x[0].as_str().cmp(&y[0].as_str()));
    let cutoff_s = a["model"]["knowledge_cutoff"].as_str().unwrap_or("1970-01-01");
    Ok(Def {
        name: name.into(),
        stage: a["stage"].as_str().unwrap_or("").into(),
        model: a["model"]["id"].as_str().unwrap_or("").into(),
        cutoff: pipeline_wire::parse_iso(cutoff_s)?,
        skills,
        caps: a["caps"].as_array().cloned().unwrap_or_default().iter().filter_map(|s| s.as_str().map(String::from)).collect(),
        budget: serde_json::to_value(&t["budget"]).map_err(|e| e.to_string())?,
        hash: hex(&bytes),
        toml_bytes: bytes,
        skills_hash: id_of(&Value::Array(pairs)).hex(),
    })
}

#[derive(Clone)]
enum Work {
    Triage { doc: Value },
    Analyze { doc: Value },
    Strategize { doc: Value, hyp: Value },
}

struct Shared {
    work: Mutex<(VecDeque<Work>, usize)>, // queue, in flight
    cv: Condvar,
    spent: Mutex<f64>,
    log: Mutex<()>,
}

/// H (Definition PL-4.1): the latest fixity time of the inputs and the
/// model's knowledge cutoff, rounded up to the second (PL-D11: the plan
/// guard renders H at whole seconds; rounding up keeps H ≥ every f_d).
fn horizon(doc: &Value, def: &Def) -> i64 {
    let f = doc["fixed_at"].as_str().and_then(|s| pipeline_wire::parse_iso(s).ok()).unwrap_or(0);
    let h = f.max(def.cutoff);
    let rem = h.rem_euclid(NS_PER_S);
    if rem == 0 {
        h
    } else {
        h - rem + NS_PER_S
    }
}

fn run(cfg: RunCfg) -> Result<(), String> {
    let defs: BTreeMap<&str, Def> = [
        ("triage", load_def(&cfg.skills, "triage")?),
        ("analyze", load_def(&cfg.skills, "analyst")?),
        ("strategize", load_def(&cfg.skills, "quant")?),
    ]
    .into_iter()
    .collect();
    for (stage, d) in &defs {
        if d.stage != *stage {
            return Err(format!("the {} definition is for stage {}, not {}", d.name, d.stage, stage));
        }
    }
    let docs = read_topic(&cfg.queue, "docs");
    let shared = Arc::new(Shared {
        work: Mutex::new((VecDeque::new(), 0)),
        cv: Condvar::new(),
        spent: Mutex::new(0.0),
        log: Mutex::new(()),
    });
    // Spend already recorded by earlier runs of the same queue.
    *shared.spent.lock().unwrap() = read_topic(&cfg.queue, "jobs").iter().filter_map(|j| j["cost_usd"].as_f64()).sum();
    {
        let mut w = shared.work.lock().unwrap();
        for d in docs {
            if cfg.only.is_empty() || cfg.only.iter().any(|o| Some(o.as_str()) == d["source_id"].as_str()) {
                w.0.push_back(Work::Triage { doc: d });
            }
        }
    }
    let cfg = Arc::new(cfg);
    let defs = Arc::new(defs);
    let mut handles = vec![];
    for _ in 0..cfg.parallel.max(1) {
        let (cfg, defs, shared) = (cfg.clone(), defs.clone(), shared.clone());
        handles.push(std::thread::spawn(move || loop {
            let item = {
                let mut w = shared.work.lock().unwrap();
                loop {
                    if let Some(i) = w.0.pop_front() {
                        w.1 += 1;
                        break Some(i);
                    }
                    if w.1 == 0 {
                        break None;
                    }
                    w = shared.cv.wait(w).unwrap();
                }
            };
            let Some(item) = item else {
                shared.cv.notify_all();
                return;
            };
            let next = match step(&cfg, &defs, &shared, item) {
                Ok(n) => n,
                Err(e) => {
                    let _g = shared.log.lock().unwrap();
                    eprintln!("job error: {}", e);
                    vec![]
                }
            };
            let mut w = shared.work.lock().unwrap();
            for n in next {
                match n {
                    // The longest jobs start first, so the campaign ends sooner.
                    Work::Strategize { .. } => w.0.push_front(n),
                    _ => w.0.push_back(n),
                }
            }
            w.1 -= 1;
            shared.cv.notify_all();
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    eprintln!("run done; OpenAI spend ${:.3}", *shared.spent.lock().unwrap());
    Ok(())
}

fn failure(cfg: &RunCfg, doc: &Value, stage: &str, reason: &str, detail: &str, job: &str) -> Result<(), String> {
    append(
        &cfg.queue,
        "failures",
        &json!({"post_id": doc["source_id"], "handle": doc["author"]["handle"], "doc_id": doc["id"], "stage": stage, "reason": reason, "detail": detail, "job_id": job, "r": iso(now_ns())}),
    )
}

fn step(cfg: &RunCfg, defs: &BTreeMap<&str, Def>, shared: &Shared, item: Work) -> Result<Vec<Work>, String> {
    let (stage, doc, hyp) = match &item {
        Work::Triage { doc } => ("triage", doc.clone(), None),
        Work::Analyze { doc } => ("analyze", doc.clone(), None),
        Work::Strategize { doc, hyp } => ("strategize", doc.clone(), Some(hyp.clone())),
    };
    let def = &defs[stage];
    let mut inputs = vec![doc["id"].as_str().unwrap_or("").to_string()];
    if let Some(h) = &hyp {
        inputs.push(h["id"].as_str().unwrap_or("").to_string());
    }
    let job_id = id_of(&json!({"stage": stage, "inputs": inputs, "definition_hash": def.hash, "skills_hash": def.skills_hash, "model": def.model})).hex();
    let h = horizon(&doc, def);
    let dir = cfg.queue.join("jobs").join(&job_id);
    let out = dir.join("out");
    let status_path = out.join("status.json");
    let mut cost = 0.0;
    // A job that failed on the model's rate limit waits and runs again
    // (PL-10: jobs wait in the queue, never fail for a 429), at most three
    // times; the earlier status is kept beside it.
    let st_prev: Value = fs::read(&status_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    if st_prev["reason"] == "model_unavailable" {
        let attempts_path = dir.join("attempts");
        let attempts: u32 = fs::read_to_string(&attempts_path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(1);
        if attempts < 3 {
            fs::rename(&status_path, out.join(format!("status.{}.json", attempts))).map_err(|e| e.to_string())?;
            fs::write(&attempts_path, (attempts + 1).to_string()).map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_secs(30 * attempts as u64));
        }
    }
    if !status_path.exists() {
        // The OpenAI guard: no job starts once the spend reaches the cap
        // less the dearest job's own cap.
        let spent = *shared.spent.lock().unwrap();
        let job_cap = def.budget["money_usd"].as_f64().unwrap_or(0.25);
        if spent + job_cap > cfg.cap_usd {
            failure(cfg, &doc, stage, "budget", &format!("the OpenAI guard: ${:.2} spent of ${:.2}", spent, cfg.cap_usd), &job_id)?;
            return Ok(vec![]);
        }
        let inp = dir.join("in");
        fs::create_dir_all(inp.join("docs")).map_err(|e| e.to_string())?;
        fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let job = json!({
            "record": "Job", "id": job_id, "stage": stage, "agent": def.name, "model": def.model, "caps": def.caps,
            "budget": def.budget, "h": iso(h), "knowledge_cutoff": iso(def.cutoff), "lineage": "", "campaign": cfg.campaign,
            "inputs": inputs, "definition_hash": def.hash, "skills_hash": def.skills_hash,
        });
        fs::write(inp.join("job.json"), serde_json::to_vec_pretty(&job).unwrap()).map_err(|e| e.to_string())?;
        fs::write(inp.join("definition.toml"), &def.toml_bytes).map_err(|e| e.to_string())?;
        for s in &def.skills {
            let d = inp.join("skills").join(s);
            fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            fs::copy(cfg.skills.join("skills").join(s).join("SKILL.md"), d.join("SKILL.md")).map_err(|e| e.to_string())?;
        }
        let did = doc["id"].as_str().unwrap_or("doc");
        fs::write(inp.join("docs").join(format!("{}.json", did)), serde_json::to_vec_pretty(&doc).unwrap()).map_err(|e| e.to_string())?;
        fs::write(inp.join("docs").join("closure.json"), json!({"inputs": [did], "closure": [did]}).to_string()).map_err(|e| e.to_string())?;
        if let Some(hy) = &hyp {
            fs::write(inp.join("hypothesis.json"), serde_json::to_vec_pretty(hy).unwrap()).map_err(|e| e.to_string())?;
        }
        let token = mint(cfg, &job_id, &def.name, &def.caps, h, &def.budget)?;
        fs::write(inp.join("token"), &token).map_err(|e| e.to_string())?;
        if stage == "strategize" {
            // Provenance at job start (PL-7.2), through the orchestrator's own token.
            let _ = provenance(cfg, &doc, hyp.as_ref().unwrap(), h);
        }
        let wall = def.budget["wall_s"].as_f64().unwrap_or(1500.0) + 120.0;
        let log = fs::File::create(dir.join("harness.log")).map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        let mut child = Command::new(&cfg.python)
            .args(["-m", "agents_harness"])
            .current_dir(&cfg.harness_dir)
            .env("JOB_DIR", &dir)
            .env("OPENAI_API_KEY", &cfg.openai_key)
            .env("OPENAI_BASE_URL", "https://api.openai.com/v1")
            .env("ABT_GATEWAY", &cfg.gateway)
            .env("PIPELINE_JOB_ID", &job_id)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .spawn()
            .map_err(|e| format!("the harness: {}", e))?;
        let code = loop {
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break s.code();
            }
            if t0.elapsed().as_secs_f64() > wall {
                let _ = child.kill();
                let _ = child.wait();
                fs::write(
                    &status_path,
                    json!({"state": "failed", "reason": "budget", "retryable": false, "detail": "wall time: killed by the runner"}).to_string(),
                )
                .map_err(|e| e.to_string())?;
                break Some(2);
            }
            std::thread::sleep(Duration::from_millis(500));
        };
        let c: Value = fs::read(out.join("cost.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        cost = c["money_usd"].as_f64().unwrap_or(0.0);
        *shared.spent.lock().unwrap() += cost;
        if !status_path.exists() {
            fs::write(
                &status_path,
                json!({"state": "failed", "reason": "internal", "retryable": false, "detail": format!("the harness exited {:?} without a status", code)}).to_string(),
            )
            .map_err(|e| e.to_string())?;
        }
        let st: Value = fs::read(&status_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        append(
            &cfg.queue,
            "jobs",
            &json!({"record": "Job", "id": job_id, "stage": stage, "agent": def.name, "model": def.model, "inputs": inputs, "h": iso(h), "state": st["state"], "failure": st, "exit": code, "wall_s": t0.elapsed().as_secs_f64(), "cost_usd": cost, "cost": c, "campaign": cfg.campaign, "post_id": doc["source_id"], "r": iso(now_ns())}),
        )?;
        let spent = *shared.spent.lock().unwrap();
        let _g = shared.log.lock().unwrap();
        eprintln!(
            "{} {} post {} -> {} ({:.0} s, ${:.4}; OpenAI spend ${:.3})",
            stage,
            &job_id[..12],
            doc["source_id"].as_str().unwrap_or(""),
            st["state"].as_str().unwrap_or("?"),
            t0.elapsed().as_secs_f64(),
            cost,
            spent
        );
    }
    let _ = cost;
    collect(cfg, stage, &job_id, &doc, h)
}

/// The job's outputs onto the queue's topics, and what runs next.
fn collect(cfg: &RunCfg, stage: &str, job_id: &str, doc: &Value, h: i64) -> Result<Vec<Work>, String> {
    let dir = cfg.queue.join("jobs").join(job_id);
    let st: Value = fs::read(dir.join("out/status.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let recs: Vec<Value> = fs::read_to_string(dir.join("out/records.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let r = iso(now_ns());
    if st["state"] != "done" {
        failure(cfg, doc, stage, st["reason"].as_str().unwrap_or("failed"), st["detail"].as_str().unwrap_or(""), job_id)?;
        return Ok(vec![]);
    }
    // Idempotent: a job collected before is not collected twice.
    let marker = dir.join("collected");
    let first = !marker.exists();
    let mut next = vec![];
    match stage {
        "triage" => {
            let t = recs.iter().find(|x| x["record"] == "Triage").cloned().unwrap_or(Value::Null);
            let verdict = t["verdict"].as_str().unwrap_or("").to_lowercase();
            if first {
                let mut t2 = t.clone();
                t2["doc_id"] = doc["id"].clone();
                t2["job_id"] = json!(job_id);
                t2["r"] = json!(r);
                t2["id"] = json!(id_of(&json!({"doc_id": doc["id"], "doc_version": 1, "model": t["model"], "prompt_hash": t["prompt_hash"], "verdict": verdict, "r": r})).hex());
                append(&cfg.queue, "triage", &t2)?;
            }
            if verdict == "candidate" || cfg.gate == "none" {
                next.push(Work::Analyze { doc: doc.clone() });
            } else if first {
                failure(
                    cfg,
                    doc,
                    "triage",
                    &format!("triage_{}", if verdict.is_empty() { "none" } else { &verdict }),
                    "not a tradeable candidate (the relevance gate, PL-5.5)",
                    job_id,
                )?;
            }
        }
        "analyze" => {
            let mut hyps: Vec<Value> = recs.into_iter().filter(|x| x["record"] == "Hypothesis").take(3).collect();
            for hy in hyps.iter_mut() {
                hy["h"] = json!(iso(h));
                hy["doc_ids"] = json!([doc["id"]]);
                hy["job_id"] = json!(job_id);
                hy["r"] = json!(r);
                hy["id"] = json!(id_of(
                    &json!({"doc_ids": [doc["id"]], "h": h, "statement": hy["statement"], "universe": hy["universe"], "signal": hy["signal"], "agent": hy["agent"], "model": hy["model"]})
                )
                .hex());
                if first {
                    append(&cfg.queue, "hypotheses", hy)?;
                }
            }
            // Today's cut: the most confident hypothesis of a post goes to the quant (debt).
            hyps.sort_by(|a, b| b["confidence"].as_f64().unwrap_or(0.0).partial_cmp(&a["confidence"].as_f64().unwrap_or(0.0)).unwrap());
            match hyps.into_iter().next() {
                Some(hy) => next.push(Work::Strategize { doc: doc.clone(), hyp: hy }),
                None if first => failure(cfg, doc, "analyze", "no_hypothesis", "the analyst found no falsifiable hypothesis", job_id)?,
                None => {}
            }
        }
        _ => {
            if first {
                match recs.into_iter().find(|x| x["record"] == "Submission") {
                    Some(mut s) => {
                        s["job_id"] = json!(job_id);
                        s["post_id"] = doc["source_id"].clone();
                        s["h"] = json!(iso(h));
                        s["r"] = json!(r);
                        s["campaign"] = json!(cfg.campaign);
                        append(&cfg.queue, "submissions", &s)?;
                    }
                    None => failure(cfg, doc, "strategize", "no_submission", "the job ended done without a Submission record", job_id)?,
                }
            }
        }
    }
    if first {
        fs::write(marker, b"").map_err(|e| e.to_string())?;
    }
    Ok(next)
}

fn mint(cfg: &RunCfg, job: &str, agent: &str, caps: &[String], h: i64, budget: &Value) -> Result<String, String> {
    let o = Command::new(&cfg.gateway_bin)
        .args(["token", "mint", "--key"])
        .arg(&cfg.signing_key)
        .args(["--job", job, "--agent", agent, "--caps", &caps.join(","), "--h", &iso(h), "--ttl", "2h"])
        .args([
            "--plan-runs",
            &budget["plan_runs"].as_u64().unwrap_or(0).to_string(),
            "--grid-points",
            &budget["grid_points"].as_u64().unwrap_or(0).to_string(),
        ])
        .output()
        .map_err(|e| format!("token mint: {}", e))?;
    if !o.status.success() {
        return Err(format!("token mint: {}", String::from_utf8_lossy(&o.stderr)));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn provenance(cfg: &RunCfg, doc: &Value, hyp: &Value, h: i64) -> Result<(), String> {
    let token = mint(cfg, "orchestrator", "orchestrator", &["provenance.write".into()], h, &json!({}))?;
    let body = json!({"documents": [doc], "hypotheses": [hyp]});
    let c = reqwest::blocking::Client::builder().timeout(Duration::from_secs(60)).build().map_err(|e| e.to_string())?;
    let r = c
        .post(format!("{}/v1/provenance", cfg.gateway))
        .header("Authorization", format!("Biscuit {}", token))
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let st = r.status();
    let txt = r.text().unwrap_or_default();
    append(
        &cfg.queue,
        "provenance",
        &json!({"doc_id": doc["id"], "hypothesis_id": hyp["id"], "status": st.as_u16(), "answer": txt.chars().take(400).collect::<String>(), "r": iso(now_ns())}),
    )
}

// ---------------------------------------------------------------- page

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn table_of(v: &Value) -> String {
    // The in-sample summary as a table: an object of objects (window -> metrics),
    // a list of rows, or a flat object of metrics.
    match v {
        Value::Object(m) if m.values().all(|x| x.is_object()) && !m.is_empty() => {
            let mut cols: Vec<String> = vec![];
            for x in m.values() {
                for k in x.as_object().unwrap().keys() {
                    if !cols.contains(k) {
                        cols.push(k.clone());
                    }
                }
            }
            let mut s = format!("<table><tr><th></th>{}</tr>", cols.iter().map(|c| format!("<th>{}</th>", esc(c))).collect::<String>());
            for (k, x) in m {
                s += &format!("<tr><th>{}</th>{}</tr>", esc(k), cols.iter().map(|c| format!("<td>{}</td>", cell(&x[c]))).collect::<String>());
            }
            s + "</table>"
        }
        Value::Object(m) => format!(
            "<table>{}</table>",
            m.iter().map(|(k, x)| format!("<tr><th>{}</th><td>{}</td></tr>", esc(k), cell(x))).collect::<String>()
        ),
        Value::Array(a) if a.iter().all(|x| x.is_object()) && !a.is_empty() => {
            let cols: Vec<String> = a[0].as_object().unwrap().keys().cloned().collect();
            let mut s = format!("<table><tr>{}</tr>", cols.iter().map(|c| format!("<th>{}</th>", esc(c))).collect::<String>());
            for x in a {
                s += &format!("<tr>{}</tr>", cols.iter().map(|c| format!("<td>{}</td>", cell(&x[c]))).collect::<String>());
            }
            s + "</table>"
        }
        other => format!("<pre>{}</pre>", esc(&serde_json::to_string_pretty(other).unwrap_or_default())),
    }
}

fn cell(v: &Value) -> String {
    match v {
        Value::Number(n) => n
            .as_f64()
            .map(|f| if f.fract() == 0.0 && f.abs() < 1e12 { format!("{}", f as i64) } else { format!("{:.4}", f) })
            .unwrap_or_default(),
        Value::String(s) => esc(s),
        Value::Null => String::new(),
        other => esc(&other.to_string()),
    }
}

fn page(queue: &Path, out: &Path) -> Result<(), String> {
    let docs = read_topic(queue, "docs");
    let triage = read_topic(queue, "triage");
    let hyps = read_topic(queue, "hypotheses");
    let subs = read_topic(queue, "submissions");
    let fails = read_topic(queue, "failures");
    let jobs = read_topic(queue, "jobs");
    let spend_x: f64 = read_topic(queue, "spend").iter().filter_map(|s| s["cost_usd"].as_f64()).sum();
    let spend_ai: f64 = jobs.iter().filter_map(|j| j["cost_usd"].as_f64()).sum();
    let mut by_post: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut rows = String::new();
    let mut docs_sorted = docs.clone();
    docs_sorted.sort_by(|a, b| a["fixed_at"].as_str().cmp(&b["fixed_at"].as_str()));
    for d in &docs_sorted {
        let pid = d["source_id"].as_str().unwrap_or("");
        let did = d["id"].as_str().unwrap_or("");
        let verdict = triage.iter().rev().find(|t| t["doc_id"] == did).and_then(|t| t["verdict"].as_str()).unwrap_or("");
        let hyp = hyps.iter().find(|h| h["doc_ids"][0] == did).and_then(|h| h["statement"].as_str()).unwrap_or("");
        let outcome = if let Some(s) = subs.iter().find(|s| s["post_id"] == pid) {
            format!(
                "<a href=\"#s-{}\">submission {}</a>",
                esc(pid),
                esc(&s["id"].as_str().unwrap_or("").chars().take(12).collect::<String>())
            )
        } else if let Some(f) = fails.iter().rev().find(|f| f["post_id"] == pid) {
            format!(
                "<b>{}</b> ({}): {}",
                esc(f["reason"].as_str().unwrap_or("")),
                esc(f["stage"].as_str().unwrap_or("")),
                esc(&f["detail"].as_str().unwrap_or("").chars().take(300).collect::<String>())
            )
        } else {
            "running or not reached".to_string()
        };
        by_post.entry(pid.to_string()).or_default().push(outcome.clone());
        rows += &format!(
            "<tr><td>{}</td><td>@{}</td><td><a href=\"{}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
            esc(d["fixed_at"].as_str().unwrap_or("").get(..19).unwrap_or("")),
            esc(d["author"]["handle"].as_str().unwrap_or("")),
            esc(d["attachments"][0]["url"].as_str().unwrap_or("")),
            esc(pid),
            esc(verdict),
            esc(hyp),
            outcome,
            esc(d["via"].as_str().unwrap_or(""))
        );
    }
    let mut subs_html = String::new();
    for s in &subs {
        let pid = s["post_id"].as_str().unwrap_or("");
        let insample: Value = s["insample"].as_str().and_then(|t| serde_json::from_str(t).ok()).unwrap_or_else(|| s["insample"].clone());
        let metrics = if insample.get("metrics").is_some() { insample["metrics"].clone() } else { insample.clone() };
        subs_html += &format!(
            "<section id=\"s-{pid}\"><h2>Post {pid} (@{handle})</h2><dl><dt>H</dt><dd>{h}</dd><dt>Submission</dt><dd>{id}</dd><dt>Program hash</dt><dd>{ph}</dd><dt>Plan hash</dt><dd>{plh}</dd><dt>Lineage</dt><dd>{lin}</dd><dt>Trials used</dt><dd>{tr}</dd><dt>Model, cost</dt><dd>{model}, ${cost:.4}</dd></dl>\
             <h3>In-sample</h3>{ins}<h3>Verdicts</h3>{ver}<h3>Program</h3><pre>{prog}</pre><h3>Development plan</h3><pre>{plan}</pre><h3>Rationale</h3><pre>{rat}</pre></section>\n",
            pid = esc(pid),
            handle = esc(docs.iter().find(|d| d["source_id"] == pid).and_then(|d| d["author"]["handle"].as_str()).unwrap_or("")),
            h = esc(s["h"].as_str().unwrap_or("")),
            id = esc(s["id"].as_str().unwrap_or("")),
            ph = esc(s["program_hash"].as_str().unwrap_or("")),
            plh = esc(s["plan_hash"].as_str().unwrap_or("")),
            lin = esc(s["lineage"].as_str().unwrap_or("")),
            tr = cell(&s["trials_used"]),
            model = esc(s["model"].as_str().unwrap_or("")),
            cost = s["cost_usd"].as_f64().unwrap_or(0.0),
            ins = table_of(&metrics),
            ver = table_of(&insample.get("verdicts").cloned().unwrap_or(Value::Null)),
            prog = esc(s["program_text"].as_str().unwrap_or("")),
            plan = esc(s["plan_text"].as_str().unwrap_or("")),
            rat = esc(s["rationale"].as_str().unwrap_or("")),
        );
    }
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Seed campaign</title>\
         <style>body{{font:14px/1.45 system-ui,sans-serif;margin:16px;max-width:1200px;color:#111;background:#fff}}table{{border-collapse:collapse;margin:8px 0}}td,th{{border:1px solid #ccc;padding:3px 6px;text-align:left;vertical-align:top}}pre{{background:#f5f5f5;padding:8px;overflow-x:auto;white-space:pre-wrap}}section{{border-top:2px solid #333;margin-top:24px}}dt{{font-weight:600}}dd{{margin:0 0 4px 0;font-family:monospace}}</style></head><body>\
         <h1>Seed campaign: submissions</h1><p>Built {built}. {nd} posts, {nt} triage records, {nh} hypotheses, <b>{ns} submissions</b>, {nf} reasoned failures. Spend: X (through xAI) ${sx:.2}, OpenAI ${sa:.2}. Private: X content is shown here for the owner only, never republished. Nothing trades.</p>\
         <h2>Posts</h2><table><tr><th>Created (fixity)</th><th>Account</th><th>Post</th><th>Triage</th><th>Hypothesis</th><th>Outcome</th><th>Text via</th></tr>{rows}</table>{subs}</body></html>",
        built = iso(now_ns()),
        nd = docs.len(),
        nt = triage.len(),
        nh = hyps.len(),
        ns = subs.len(),
        nf = fails.len(),
        sx = spend_x,
        sa = spend_ai,
        rows = rows,
        subs = subs_html,
    );
    if let Some(p) = out.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    fs::write(out, html).map_err(|e| e.to_string())?;
    eprintln!("{}: {} posts, {} submissions", out.display(), docs.len(), subs.len());
    Ok(())
}

# Contracts between the pipeline's parts (iteration 1, Oct 19, 2026)

What one crate promises another, fixed before the crates are written so that they can be written apart. The plan (`pipeline.md`, `PL-n`) is the authority; this page names the exact shapes where the plan names only the idea. Decisions here are numbered `PL-Dn` and are listed again in `docs/iterations/01-mvp.md`.

## 1. The workspace

| Path | Crate / dir | Binary | Depends on |
| --- | --- | --- | --- |
| `crates/pipeline-wire` | records, ids, time, topics (done) | — | `abt-wire` |
| `crates/ingest-core` | WAL, object store, index (`redb`), cursors, the control topic, the `Source` trait, the broker client, the `file` adapter, `ingest bench`, `ingest replay` | `ingest` | `pipeline-wire`, `ingest-extract`, `rdkafka` |
| `crates/ingest-extract` | PL-5.3 extraction, pure functions: normalise, segments, entities (symbol table), flags (exact, SimHash, injection, spam) | — | `pipeline-wire` |
| `crates/ingest-x` | the X API client (P1 poll, `since_id`), the follower with tiers and the spending guard, extraction of a post to `Document` v1, compliance tombstones, `ingest follow …` | (in `ingest`) | `ingest-core`, `ingest-extract` |
| `crates/ingest-fetch` | the SSRF-safe fetcher, HTML and PDF parsers in seccomp-confined children, versions `n+1` and child documents | `ingest-fetch` | `pipeline-wire`, `rdkafka` |
| `crates/agents-orchestrator` | jobs, Biscuit tokens, the sandbox pool, budgets, DLQ, campaigns; the egress proxy | `orchestrator`, `egress-proxy`, `pipeline` | `pipeline-wire`, `rdkafka`, `biscuit-auth` |
| `agents-harness/` | Python: the OpenAI Agents SDK agents, the tools over the gateway's JSON API | `python -m agents_harness` | — |
| `agents-skills/` | `definitions/*.toml`, `skills/*/SKILL.md` | — | — |
| `crates/abt-gateway` | the JSON tool API, the `abt-wire` face, clamps, plan guards, the kernel-pool queue, `pipeline.*` provenance, audit, the forward scheduler | `abt-gateway` | `pipeline-wire`, `abt-wire`, `biscuit-auth`, `axum`; tests: `abt-data`, `abt-kernel`, `abt-query` |
| `crates/abt-report` | the static site | `abt-report` | `pipeline-wire`, `abt-wire`, `rdkafka` |
| `deploy/` | `compose.yaml`, Dockerfiles, `up.sh` | — | — |

Every crate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; the harness `ruff check` and `pytest`. Rust 2021, `rustfmt.toml` as the backtester's (`max_width = 200`). Tests that need a broker read `PIPELINE_BROKERS` (e.g. `127.0.0.1:19092`) and skip with a printed reason when it is unset; `deploy/redpanda-local.sh` starts one in Docker for them.

## 2. Records on the log

Every message is one `pipeline-wire` envelope (`encode`/`decode`), one record, under `topics::MAX_MESSAGE_BYTES`. Headers: `schema = pipeline.1`, `record = <table name>`, `id = <hex>`. Keys as `topics::TOPICS`. Ids are hex strings (PL-D1). Records are produced with the idempotent producer, `acks=all`. Consumers are idempotent by id: a redelivered message is a no-op once its effect is durable.

**PL-D2** (JSON for people and for the harness): wherever a record leaves Rust (the sandbox's `/job/in`, the site, `ingest wire decode`), it is `pipeline_wire::json::render` (ids hex, times ISO-8601, a `record` member naming the table). The harness writes records back in the same shape (ISO times accepted; `r` and ids are set by the orchestrator, never trusted from the sandbox).

## 3. The sandbox's job directory (orchestrator ⇄ harness)

```
/job/in/job.json            the Job record (render shape) plus: h (ISO), knowledge_cutoff (ISO), caps, budget, lineage, model, campaign
/job/in/token               the Biscuit token, base64, one line
/job/in/definition.toml     the agent definition (PL-6.2)
/job/in/skills/<name>/SKILL.md   the skills the definition lists, in order
/job/in/docs/<doc_id>.json  every input document and its provenance closure (parents, linked children, every version's latest)
/job/in/docs/closure.json   {"inputs": [ids], "closure": [ids in reading order]}
/job/in/hypothesis.json     strategize only: the Hypothesis record
/job/out/records.jsonl      one record per line, render shape, `id` and `r` left empty (the orchestrator computes them)
/job/out/notes.md           the notes (`notes.write`)
/job/out/cost.json          {"tokens_in", "tokens_out", "tokens_cached", "tool_calls", "wall_s", "money_usd", "model"}
/job/out/status.json        {"state": "done" | "failed", "reason": "not_testable|no_data|could_not_check|no_decisions|budget|injection|…", "retryable": bool, "detail": "…"}
```

Environment inside the sandbox: `JOB_DIR=/job`, `OPENAI_BASE_URL=http://10.200.0.1:8080/openai/v1`, `OPENAI_API_KEY=sandbox` (a placeholder; the proxy injects the real key), `ABT_GATEWAY=http://10.200.0.1:8080/gateway`, `PIPELINE_JOB_ID`. The harness exits 0 when `/job/out/status.json` is written, 2 on a budget overrun (and writes `budget`), 3 on an internal error. The orchestrator validates every record of `records.jsonl` against the stage's output list and the schema; an invalid file fails the job as `invalid_output`. **PL-D3**: the sandbox image is `absolute/harness:<skills_version>`; the job directory is a bind mount of a per-job temp dir, `/job/in` read-only, `/job/out` the only writable path besides `/tmp` (tmpfs 64 MB).

## 4. The egress proxy (sandbox ⇄ OpenAI, sandbox ⇄ gateway)

Listens on the sandbox network's gateway address (`10.200.0.1:8080`, PL-D4: a Docker bridge `absolute-sandbox` with internal routing only; the proxy is the only host the sandboxes can reach, by `iptables` on the host and by the bridge being `internal`). Routes:

| Path | To | Rule |
| --- | --- | --- |
| `/openai/v1/*` | `https://api.openai.com/v1/*` | `Authorization: Bearer <key>` injected from `/etc/absolute-pipeline/openai_api_key`; only `POST /v1/responses`, `POST /v1/chat/completions`, `GET /v1/models`; the `usage` of every response is logged per job (the `X-Pipeline-Job` header the harness sets, else the source address's job) |
| `/gateway/*` | `http://127.0.0.1:7777/*` | passed through with the request's `Authorization: Biscuit …` header |
| anything else | — | 403, logged |

Spending: the proxy keeps the day's model spend per job and in total from `usage` at the rate card of PL-8.5 (configurable `rates.toml`); past the global cap (`$50` today) it answers 402 and the orchestrator halts. The gateway's loopback port is reachable from the proxy only (bound to 127.0.0.1).

## 5. The gateway's JSON tool API (harness ⇄ gateway)

`POST http://127.0.0.1:7777/v1/<route>`, header `Authorization: Biscuit <base64>`, JSON body, JSON answer `{"ok": true, "result": …}` or `{"ok": false, "error": {"code": "…", "message": "…", "detail": …}}` with the HTTP status 200 / 400 (malformed) / 403 (the token lacks the capability or the fact) / 409 (refused by a guard) / 429 (busy, over a budget) / 502 (a layer failed). Tables are `{"columns": [names], "rows": [[…]], "truncated": bool}` with at most 500 rows (PL-D5; the agent narrows the query).

| Route | Capability | Body | Result |
| --- | --- | --- | --- |
| `catalog` | `abt.catalog` | `{"class"?: "future", "predicate"?: "root = ES"}` | a table of the registry |
| `schema` | `abt.catalog` | `{"class": "future"}` | `{"text": <the class catalog as abt data catalog prints it>}` |
| `resolve_symbols` | `abt.catalog` | `{"symbols": ["ES", "QQQ"]}` | `[{"symbol", "abt_id", "class", "root", "listed_from", "listed_to"}]` |
| `query` | `abt.query` | `{"form": {"kind": "point|latest|series|section|scan|exists|count|distinct", "relation": "future.close", "identity"?: [ids], "t"?, "t_max"?, "t0"?, "t1"?, "predicate"?, "columns"?}, "r_max"?: ISO}` | a table; `r_max := min(r_max, H)`, `as_of_seq := seq(H)` |
| `check` | `abt.check` | `{"program": text, "name": "slug"}` | `{"diagnostics": [{"code", "severity", "message", "span"}], "program_hash"?, "degrees_of_freedom"?}` |
| `contract` / `show` | `abt.check` | `{"program", "name"}` / `{"program", "name", "what"}` | `{"clauses": […]}` / `{"text", "tables"}` |
| `plan_check` | `abt.plan` | `{"plan": text, "programs": {"slug.dsl": text}}` | `{"hash", "normal_form", "trials_expected", "steps": […]}` after the guards of PL-7.1 |
| `plan_run` | `abt.plan` | as `plan_check` | `{"plan_hash", "runs": [{"step", "run_id", "status", "decisions"}], "report": <plan_report>}`; one in flight per job; counted against `plan_runs` and `grid_points` |
| `plan_report` | `abt.report` | `{"plan_hash"}` | `{"summary": table, "trials": table, "claims": table, "contract": table, "warnings": table, "biases": table, "surface"?, "subperiods"?, "contribution"?, "text": "…"}` |
| `query_run` | `abt.query_run` | `{"run_id", "text"}` (the kernel's `?`, `why`, `why not`, `trace`, `coverage`, `show`, `ledger`) | a table or `{"text"}` |
| `submit` | `abt.submit` | `{"hypothesis_id", "program_hash", "plan_hash", "program_text", "plan_text", "forward_plan_text", "claims": [..], "rationale", "insample": {...}}` | `{"submission_id", "forward_plan_hash"}`; validated as PL-6.4; one per job |
| `provenance` | `provenance.write` (the orchestrator's token only) | `{"documents": [Document render], "hypotheses": [Hypothesis render]}` | `{"written": n}` |
| `health` | — | — | `{"data", "kernels", "query": "ok"}` |

**The plan guards** (PL-6.4, PL-6.6), applied to the plan text before the query layer sees it: `program` names a program whose hash is in the token's lineage (or opens it when the lineage is empty); `holdout` is `trailing:…` with `window … .. H` (development) or `after H` (forward) where `H` is the token's `h` rendered as `YYYY-MM-DDTHH:MM:SSZ`; `budget { money 0; … }`; every `execution` block in the programs is `futures_house` or `equities_house` or one of them with one field changed in a program named for that field; the grid ≤ `grid_points`; plan executions in the job ≤ `plan_runs`; no `reveal` of a forward plan by an agent. A refusal is `409` with `code` ∈ {`holdout`, `lineage`, `budget`, `preset`, `grid`, `plan_runs`, `reveal`}.

**The Biscuit token** (PL-4.5): root key pair ed25519 at `/etc/absolute-pipeline/token_signing_key` (generated by the orchestrator on first start; public key in `token_public_key` beside it, read by the gateway). Facts: `job("<id>")`, `agent("quant")`, `cap("abt.query")` …, `h(<ns>)`, `lineage("<hex or empty>")`, `plan_runs(4)`, `grid_points(25)`, `expires(<ns>)`; check `check if time($t), expires($e), $t < $e`. The gateway adds `time(now)` and verifies with the public key; a route requires `cap("<name>")`.

## 6. `pipeline.*` in the data layer (gateway ⇄ abt-data)

Written by the gateway only, through `put` under class `pipeline` (the amendment of D-2.3): `pipeline.document`, `pipeline.hypothesis`, `pipeline.submission`, `pipeline.outcome`, `pipeline.audit`, columns as PL-7.2. Ids are `Label`s (hex). The report site reads them through the data socket with `scan`.

## 7. Skills and definitions (orchestrator ⇄ harness ⇄ skills)

`agents-skills/definitions/<agent>.toml` as PL-6.2. `agents-skills/skills/<name>/SKILL.md` with a front matter `name:` and `version:`. The skills hash of a job is BLAKE3 over the canonical JSON `[[name, BLAKE3(bytes)], …]` sorted by name; the definition hash BLAKE3 over the TOML bytes; `skills_version` (a campaign's) is the git tree hash of `agents-skills/` or a tag name. The nine skills: `house-rules` (the guideline verbatim), `content-is-data`, `triage`, `extract-hypothesis`, `abt-language`, `abt-idioms`, `abt-plans-and-claims`, `write-strategy`, `read-results`.

## 8. The report (site ⇄ everything)

`abt-report build --brokers … --data SOCKET --out DIR` renders every page of PL-7.4 into `DIR` as static HTML (no JavaScript needed to read; a little for filters), from the log's topics `docs`, `triage`, `hypotheses`, `submissions`, `outcomes`, `control` (from offset 0, each time) and the data layer's `pipeline.*` and `query.<plan>.report.*`. `nginx` serves `DIR` on the machine's port 8080, reached through an SSH tunnel (PL-D6: the security group admits SSH only; `ssh -L 8080:127.0.0.1:8080` is the site's address).

## 9. Decisions in this page

| # | Decision | Rejected |
| --- | --- | --- |
| PL-D1 | ids travel as 64-hex strings in the FlatBuffers records | a 32-byte table or array (planus boxes tables and has no fixed arrays) |
| PL-D2 | one JSON rendering (`json::render`) everywhere a record leaves Rust | serde's default (byte arrays and integers for ids and times) |
| PL-D3 | the harness image per skills version; the job directory a bind mount, `in` read-only | copying inputs through the container's stdin (no provenance closure of any size) |
| PL-D4 | the sandbox network is an internal Docker bridge whose only reachable host is the proxy | per-sandbox iptables rules (fragile under a pool of twenty) |
| PL-D5 | tables to the model are capped at 500 rows | streaming tables into a model context (the budget and the context both forbid it) |
| PL-D6 | the site is reached through an SSH tunnel today | opening 8080 to the owner's IP (the instructions admit SSH only) |
| PL-D7 | the 50 seed posts fetched through the public mirror `api.fxtwitter.com` (read-only) and written in the X API v2 shape for the `file` adapter | waiting for a bearer token (the day's gates need the campaign) |
| PL-D8 | the Databento lake staged one root at a time (the adapter holds a `--from` tree in memory; the whole lake took 31.6 GB and was killed) | a bigger machine, or an adapter change in the backtester (outside the amendments) |
| PL-D9 | **the owner's decision**: the X source through xAI's `x_search` (path P5 of PL-5.2's amendment), every response's `cost_in_usd_ticks` into the $25 guard, texts verified through the mirror | the X API bearer token (none exists), scraping x.com (its terms) |

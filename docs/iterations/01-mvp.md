# Iteration 1 · The MVP, as the owner cut it: the seed posts through the chain

Oct 19, 2026. The first iteration of `absolute-pipeline`, in the form of `absolute-backtest`'s `docs/iterations/01-foundation.md`. It began as the whole MVP of `docs/design/pipeline.md` §8 (MVP-0 to MVP-6). At 17:52 UTC the owner cut it: MVP-6, GP-6 and GP-7 out of scope; within the hour, the 50 seed posts through triage, analyst and quant on the AWS machine and visible on one page, with four non-negotiables (a program on `futures_house`, checked; one development plan through the gateway under the `H` clamp; `holdout trailing:20%`; `budget { money 0 }` with the lineage recorded) and everything else cut and listed as debt below. The guards stand: X $25, OpenAI $50, X read-only, nothing trades.

## Where the tree starts

`absolute-pipeline` at d4b1e3d held the documents only. `absolute-backtest` was at a4128ca on `spec/three-layer-split`. The owner's secrets as received: an OpenAI key and an xAI key. There is no X API bearer token. At 17:40 UTC the owner decided to read X through xAI instead (PL-D9).

## The machine

| | |
| --- | --- |
| Instance | `i-054398816027e565c`, m7i.2xlarge (8 vCPU, 32 GB, 30 GiB usable), us-east-1a, Ubuntu 24.04, kernel 7.0.0-1014-aws |
| Address | 100.59.51.177 (SSH from the owner's IP only; nothing else inbound) |
| Disk | 300 GB gp3 at the baseline (3,000 IOPS, 125 MB/s) |
| Software | Docker 28 with gVisor (`runsc`) registered, Rust 1.99.1, Python 3.12.3, memlock unlimited |
| Role | `absolute-pipeline-machine`: read on `ndlake-…`, `dbn-futures-…`, `orats-backup-…` |
| Cost | $0.4032 an hour on demand plus $0.0329 an hour for the volume: **$0.436 an hour**, about $10.50 a day |
| Recreate | `scripts/aws/provision.sh` (idempotent), `sync.sh`, `up.sh`; `down.sh` exists and was not run |

It stays running.

## Plan as run

1. **Amendments (PR #76 on `absolute-backtest`).** Q-2.1 `holdout after TIME` and `trailing:N%`/`trailing:Nmo` (E-97), the `pipeline` class (E-94), the red-flag metric rows (E-95), the house presets and the execution grammar they need (E-96). The branch moved onto the iteration-1 books commits (8161fef, d5b7380), so the hold-out erratum was renumbered E-97.
2. **The crates, written by one agent each in a worktree and merged here:** `pipeline-wire` (records, ids, topics), `abt-gateway` (the JSON tool API, the clamps, the plan guards, the kernel-pool queue, provenance, the forward scheduler, GP-4's property test with ten mutations), `agents-harness` and `agents-skills` (the nine skills, three definitions, 37 tests, three live smoke tests on `gpt-6-luna`), `abt-report` (the full static site over fixtures). The ingest, fetch and orchestrator agents stopped mid-work when the session ended; their branches are unmerged.
3. **The machine:** the Norgate S&P 500 history since 2014 and 33 Databento roots staged into one store (below), the three programs on Unix sockets in the `dedicated` profile with four kernels, the gateway, and nginx serving one page on `127.0.0.1:8080`.
4. **The cut (17:52 UTC):** `crates/pipeline-run`, one process. `fetch` gets the seed posts by id through xAI. `run` executes triage, analyst and quant as harness subprocesses over a file-backed queue of JSON records. `page` writes the one page. `deploy/run-seed.sh` starts it all.

### What the runs found, and the four fixes

| Run | Finding | Fix |
| --- | --- | --- |
| 1 | Triage passed 5 of 50 posts as `candidate` (16 commentary, 15 none, 14 claim); the relevance gate stopped the rest | PL-D12: a curated campaign records triage without gating (`--gate none`); the live feed keeps the gate |
| 1 | Two concurrent jobs interleaved one line of `triage.jsonl` | one write per record under the queue's lock |
| 2 | The analyst returned no hypothesis for 46 of 50 posts, reading "zero when the rule cannot be stated without inventing it" against house rules §8 | `extract-hypothesis` v2: a gap is a labelled assumption; zero only without a market claim |
| 3 | The quant ended `not_testable` where the analyst's labelled assumptions filled a gap; two jobs overran 400k input tokens | `write-strategy`: the labelled assumptions are the rule to test (PL-D13); the quant's input-token cap 1.2M, money cap unchanged at $0.25 (PL-D14) |
| 3 | Twenty quants at once hit the organisation's 2M tokens a minute on `gpt-6-luna`; the harness gave up after its retries | the runner requeues a rate-limited job (three attempts), parallelism 8 |
| 4 | Every plan was refused by the data layer: the quant declared `complete` bars (WF-5), and no futures version carried `reviewed`; daily programs on the presets failed WF-14 (`orders moo_moc` is intraday); one program declared a `commission` relation that does not exist | the owner confirmed the data valid: every current future and equity version marked `reviewed = complete` (PL-D16); `write-strategy` gives the daily variant `with { orders moo }` in a strategy named for it, and says costs come from the preset (PL-D17); the quant jobs rerun |

<!-- RESULTS -->

## Gates

| Gate | Status today |
| --- | --- |
| GP-1 (ingest budgets on 100,000 recorded responses) | **not run.** The ingest crates are unmerged (their agent stopped) and the corpus was never recorded. The cut replaced the hot path. |
| GP-2 (48 h poll of the seed list) | **not started.** No X bearer token exists, and the xAI follower (P5) is unwritten. |
| GP-3 (triage and analyst schemas, injection, budget overrun) | **partly, in tests only.** The harness's tests cover the injected document, the budget exit and the schemas against fakes. The orchestrator half is unmerged. |
| GP-4 (the honesty property) | **green at the amendments' pin (9d69523)**, on this Mac: the gateway's `gp4.rs`, 3 of 3, a property test over the query forms in process and through the tool API, and the guards' refusal codes. The bump from a4128ca first failed every integration test at its fixture (`equities_house` on a daily clock is refused by WF-14); the fixture is now `idea_moc` on `equities_house with { orders moc }`. The API's 3 and the scheduler's 2 pass; the load test fails one run in three, one plan of eight concurrent jobs refused with code `plan` (open). The ten mutations were not rerun. |
| GP-5 (R8L on the chosen model; the seed campaign) | **the campaign ran; the usability task did not.** The R8L task needs the R8L bundle, which is not on the machine. The campaign's results are above. |
| GP-6, GP-7 | out of scope (the owner's cut) |

## Decisions

| # | Decision | Rejected |
| --- | --- | --- |
| PL-D1 | ids travel as 64-hex strings in the FlatBuffers records | a 32-byte table or array (planus boxes tables and has no fixed arrays) |
| PL-D2 | one JSON rendering (`json::render`) wherever a record leaves Rust | serde's default rendering |
| PL-D3 | the harness image per skills version; the job directory a bind mount, `in` read-only | inputs through stdin |
| PL-D4 | the sandbox network an internal bridge whose only host is the proxy | per-sandbox iptables rules |
| PL-D5 | tables to the model capped at 500 rows | streaming tables into a context |
| PL-D6 | the site reached through an SSH tunnel | opening 8080 to the owner's IP |
| PL-D7 | the seed posts' exact text from the public mirror `api.fxtwitter.com` (read-only) | waiting for a bearer token |
| PL-D8 | the Databento lake staged one root at a time: the adapter holds a `--from` tree in memory, and the whole lake took 31.6 GB and was killed | a bigger machine, or an adapter change outside the amendments |
| PL-D9 | **the owner's:** X through xAI's `x_search`; every response's `cost_in_usd_ticks` in the X guard; each text checked against the mirror | the X API (no token), scraping x.com (its terms) |
| PL-D10 | the historical bulk load under `eatmydata` (fsync a no-op) with one `sync` at the end: the ingest was fsync-bound at about 93 journal commits a second on EBS; a crash mid-load is recovered by staging again from the landed objects (ST-7.5) | provisioned IOPS (the latency, not the bandwidth, was the cost) |
| PL-D11 | `H` rounded up to the second: the plan guard renders `H` at whole seconds, and rounding up keeps `H ≥ f_d` | rounding down (`H` would precede the post's fixity time) |
| PL-D12 | a curated campaign records triage without gating it; the live feed keeps the gate | gating the seed posts (5 of 50 would reach the analyst) |
| PL-D13 | the quant tests the hypothesis's labelled assumptions as the rule; `not_testable` is for missing data | `not_testable` for every gap (no seed post would be tested) |
| PL-D14 | the quant's input-token cap 1.2M; `money_usd` $0.25 stays the binding cap | 400k (two jobs overran it with the skills cached) |
| PL-D15 | the most confident hypothesis of a post goes to the quant today | all three (the hour) |
| PL-D16 | **the owner's:** every current future and equity version marked `reviewed = complete` in the registry and the manifests (`deploy/mark-reviewed.py --all`), the services stopped. A hot version's id hashes its blocks and is unchanged; the eight Parquet versions' ids hashed their old attributes | re-staging with the attribute (hours) |
| PL-D17 | on a daily clock the presets' `orders moo_moc` (intraday, refused by WF-14) becomes the single-field variant `with { orders moo }` or `{ orders moc }`, in a strategy named for it, as the preset guard admits | changing the guideline's presets |
| PL-D120 to PL-D131 | the report site's decisions, in `crates/abt-report/README.md` and its agent's commits | |

## Owner inputs (the default taken)

1. **The X API.** There is no bearer token; the owner chose xAI (PL-D9). Whether the X console shows `since_id` on list timelines or a public `post.create` in the Activity API is unknown without the console. Default: neither is used.
2. **The data's review.** The owner said the data is valid; every relation is marked `reviewed = complete` (PL-D16). Revoking it means restoring the `.bak` files beside the registry and the manifests.
3. **The quant's model.** The quant stays on `gpt-6-luna`; the R8L usability task that would decide the move to `gpt-6.1-sol` has not run.
4. **The organisation's OpenAI rate limit:** 2,000,000 tokens and 5,000 requests a minute on `gpt-6-luna`. Twenty quants at once reach it. Raising it is the owner's call; default: 8 at once.
5. **The knowledge cutoff of `gpt-6-luna`:** May 18, 2026, from OpenAI's model page. It sets `H` for the two oldest seed posts.

## Debt

Each cut made at 17:52, and what it blocks:

| Cut | Instead, today | Blocks |
| --- | --- | --- |
| Redpanda and the topics of PL-4.3 | a single process with a file-backed queue: JSON-lines files under `/srv/absolute/queue`, one synced write per record | replay from an offset, consumer groups, GP-3's broker half |
| gVisor sandboxes and the warm pool (PL-6.7) | the harness as a subprocess of `pipeline-run`, as the `ubuntu` user | the sandboxing guarantee: a compromised harness has the host user's reach |
| the egress proxy (PL-4.5) | the OpenAI key in the harness's environment; the spend summed from each job's `cost.json` into the $50 guard | the key never entering the sandbox |
| FlatBuffers records on the log | JSON records in `json::render`'s shape | PL-P8 on the log |
| the forward scheduler (PL-7.3) | none: submissions have no reveal yet | forward evidence, the leaderboard's order |
| the report site (PL-7.4) | one static page of submissions with program, plan, in-sample table and `H` | GP-6 |
| ingest: WAL, object store, X follower, fetcher, extraction flags, compliance | the posts fetched once by id; links not fetched; injection and spam flags at 0 | GP-1, GP-2, the link enrichment of PL-5.4 |
| the orchestrator: Biscuit tokens per job (kept, minted by the gateway), budgets per stage and campaign, the DLQ, `pipeline campaign` | `pipeline-run run`: per-job caps by the harness, the global cap by the runner, failures as records | GP-3's orchestrator half |
| two of three hypotheses per post | the most confident one (PL-D15) | the analyst's full fan-out |
| the Databento lake | 33 of 59 roots staged; GC, CL, ZN, 6E, SI, 6J, HG, ZF and the larger roots not; 6M failed (duplicate `point_value` rows in its definitions) | ideas on those roots end `no_data` |
| the store's attributes | `reviewed = complete` written into the registry and the manifests by hand (PL-D16); eight Parquet versions' ids no longer hash their attributes | re-stage those relations with `--attributes reviewed=complete` |
| the ingest, fetch and orchestrator branches | unmerged worktrees `iter/01-mvp-{ingest,fetch,orchestrator}` with their commits | everything they own |

## Money spent today

| Service | Amount | Guard |
| --- | --- | --- |
| X, through xAI | $1.58: $0.40 for this morning's probe, $1.18 for the fetch of the 50 posts | $25, hard |
| OpenAI | <!-- OPENAI --> | $50, hard |
| AWS | <!-- AWS --> | none set |

## The trace at the final tree

`absolute-backtest`'s `docs/spec/TRACE.md` on the amendments branch (9d69523): 180 rows, 132 done, 37 partial, 11 missing, 0 contradicts.

## The pull requests and the page

- Amendments: https://github.com/KurtCoVayne/absolute-backtest/pull/76
- The pipeline: <!-- PIPELINE PR -->
- The page: run `ssh -i SECRETS/machine_ed25519 -L 8080:127.0.0.1:8080 ubuntu@100.59.51.177`, then open http://127.0.0.1:8080/.
- Rerun or resume the campaign: `scripts/aws/ssh.sh 'cd /srv/absolute/absolute-pipeline && GATE=none PARALLEL=8 bash deploy/run-seed.sh run'`. Finished jobs are reused.

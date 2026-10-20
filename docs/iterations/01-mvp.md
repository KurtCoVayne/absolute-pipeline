# Iteration 1 · The MVP, as the owner cut it: the seed posts through the chain

Oct 19, 2026. The first iteration of `absolute-pipeline`, in the form of `absolute-backtest`'s `docs/iterations/01-foundation.md`. It began as the whole MVP of `docs/design/pipeline.md` §8 (MVP-0 to MVP-6). At 17:52 UTC the owner cut it: MVP-6, GP-6 and GP-7 out of scope; within the hour, the 50 seed posts through triage, analyst and quant on the AWS machine and visible on one page, with four non-negotiables (a program on `futures_house`, checked; one development plan through the gateway under the `H` clamp; `holdout trailing:20%`; `budget { money 0 }` with the lineage recorded) and everything else cut and listed as debt below. The guards stand: X $25, OpenAI $50, X read-only, nothing trades.

## Where the tree starts

`absolute-pipeline` at d4b1e3d held the documents only. `absolute-backtest` was at a4128ca on `spec/three-layer-split`. The owner's secrets as received: an OpenAI key and an xAI key. There is no X API bearer token. At 17:40 UTC the owner decided to read X through xAI instead (PL-D9).

## The machine

| | |
| --- | --- |
| Instance | `i-054398816027e565c`, provisioned as an m7i.2xlarge on Oct 19 at 16:28 UTC, resized to an **r7i.2xlarge** (8 vCPU, 64 GB, 61 GB usable) at 20:13 when the data service was killed for memory; us-east-1a, Ubuntu 24.04 |
| Address | Elastic IP **54.157.116.42**, and **https://backtests.signoscapital.com** (an A record at GoDaddy). SSH open to the owner's IPs only (181.237.196.142, 190.242.101.109); 80 and 443 open, the site behind Google sign-in (PL-D29) |
| Disk | 300 GB gp3 |
| Web | Caddy (TLS, Let's Encrypt) → oauth2-proxy v7.15.5 on 127.0.0.1:4180 (Google, `@signoscapital.com` only; the OAuth consent screen Internal) → the static site's nginx on 127.0.0.1:8080: `deploy/web-up.sh` |
| Services (systemd) | `abt-data` (S&P 500 equities and the first Databento roots, `data.sock`), `abt-data-futures` (the Databento futures snapshot `store-futures-current`, `futures.sock`), `seed-site` (the site loop and nginx on port 80). `abt-data-norgate` and `abt-data-equities` exist and are disabled (below). Every data service runs with a resident budget and `MALLOC_ARENA_MAX=2` |
| Shared runs | `/usr/local/bin/abt-slot`: five slots of 10 GB (a data load reserves one); runs are the out-of-memory killer's first choice; `abt check` and `abt data query` skip the queue |
| Role | `absolute-pipeline-machine`: read on `ndlake-…`, `dbn-futures-…`, `orats-backup-…`. A write permission scoped to nine new root prefixes of `dbn-futures-…` was added for one upload and removed after it |
| Cost | m7i $0.4032 an hour, r7i $0.5292, the volume $0.0329: about **$0.56 an hour** now |
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

## The night of Oct 19–20: what changed after the cut

The hour's campaign produced no full backtest. The owner then asked for full backtests with every chart and table, and the work ran through the night.

1. **The quant moved off `gpt-6-luna`.** Eight Claude (Opus) agents wrote the strategies and ran them on the machine through `abt-slot` (22:15–04:10 UTC); at 04:10 the owner moved every backtest to OpenAI's `gpt-6.1-sol` (analyst and quant, PL-D21).
2. **Data, all of it.** The Norgate lake staged in full: every US-listed stock and ETF, current and delisted, daily from 1990, nineteen index lists, the index series (`store-full`, 30 GB). The Databento futures restaged from empty: 50 GLBX roots with sessions one per trade date, each root's five bar relations consolidated and row-counted (`store-futures`). Norgate's continuous futures (112 roots from inception) were staged and served, then retired from futures work by the owner (PL-D24). Nine more Databento roots bought at $0 (the subscription covers OHLCV) and saved to S3 with a sha256 manifest (PL-D26).
3. **The engine, twenty errata on PR #76** (E-104 to E-121, below): sessions merged into weeks before 2013, a quadratic loader, data services holding 40 GB they had freed, a 1 MiB reply that refused every run, a claim that passed negative Sharpes, grids that silently ran the default, held-out trades dropped, roll commissions never charged, rows silently lost when a relation was rebuilt after appends.
4. **The harness**: no window cap (the owner: as much history as the data holds), the Databento-only futures routing, the equity store's routing, one kernel per slot, a spend guard that counts every job.

## Results

The site lists 162 posts: the 51 seed posts, campaign 2 (33 posts from nine strategy accounts) and campaign 3 (78 posts from the owner's `links_juan_rafael.xlsx`; one more the mirror cannot serve).

| State | Posts |
| --- | --- |
| Full backtest (development plan, held-out reveal, NAV, every section of the page) | **16** |
| No testable market claim (the analyst, on `gpt-6.1-sol`) | 49 |
| The quant failed (below) | 52 |
| Queued for the quant | 45 |

The sixteen, all written by the Opus agents (22:30–04:00 UTC); returns on the fixed $1M at 0.75 % risk a trade:

| Post | Markets, window | Sharpe in / out of sample | CAGR in / out | Verdict |
| --- | --- | --- | --- | --- |
| @quantifiedstrat NQ IBS | NQ, 1999–2026 | 0.77 / 1.15 | 2.1 % / 3.1 % | holds |
| @buildalpha | ES NQ RTY YM, 1997–2026 | 0.69 / 0.96 | 6.0 % / 8.7 % | holds, long only |
| @quantifiedstrat Double Seven | ES, 1998–2026 | 0.45 / 0.96 | 1.4 % / 2.8 % | holds, long only |
| @SystematicPeter mean reversion | S&P 500 members, 2014–2026 | 0.55 / 0.88 | 8.4 % / 13.5 % | profitable, not robust |
| @quantifiedstrat 200-day MA | ES, 1998–2026 | 0.39 / 0.76 | 1.1 % / 2.1 % | marginal |
| @SystematicPeter dip buying | NQ ES RTY YM, 1997–2026 | 0.52 / 0.71 | 5.1 % / 7.1 % | holds, long only |
| @techcharts consolidation breakout | S&P 500 members | 0.49 / 0.37 | 0.4 % / 0.7 % | no edge |
| @panicsellweekly weekly 50-EMA | S&P 500 members | 0.38 / 0.32 | 4.8 % / 4.0 % | not supported |
| @quantifiedstrat Double Seven, house universe | 25 futures, 1978–2026 | 0.37 / 0.26 | 4.5 % / 2.9 % | fails |
| @quantifiedstrat turn of the month | ES, 1998–2026 | 0.42 / 0.25 | 1.0 % / 0.6 % | weak |
| four BTC chart patterns, the ZB bond rule, the ES bear-regime rule | | | | fail |

The futures among them ran on Norgate's continuous daily futures, before the owner's Databento-only rule; they are to be rerun on Databento. They predate E-112, E-114 and E-115: two held-out trade counts read 0 on the page (the verdicts give the true counts).

`gpt-6.1-sol` made no full backtest in 73 attempts (04:10–14:00 UTC). The causes: the served snapshot deleted under the service by the staging driver's rotation (fixed: the served copy is protected), two full-history runs in one 10 GB slot (fixed: one kernel), the equity position-limit invariant (open), 2-hour bars (open), ETFs and non-S&P stocks while the full equity store is off (open, E-120), single-event rules that never trade, and posts whose data the store does not hold.


## Gates

| Gate | Status at the end |
| --- | --- |
| GP-1 (ingest budgets on 100,000 recorded responses) | **not run.** The ingest crates are unmerged; the cut replaced the hot path. |
| GP-2 (48 h poll of the seed list) | **not started.** No X bearer token; the posts come by id through xAI and the public mirror. |
| GP-3 (triage and analyst schemas, injection, budget overrun) | **partly, in tests only**: the harness's 63 tests pass on the machine. |
| GP-4 (the honesty property) | **green at the amendments' pin (9d69523)**; not rerun at the final engine. |
| GP-5 (R8L on the chosen model; the seed campaign) | **the campaign ran, the usability task did not.** 16 full backtests (Opus); `gpt-6.1-sol` 0 of 73. |
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
| PL-D18 | **the owner's:** the strategies written by agents, never by the coordinator; on Oct 19 at 22:15 eight Claude (Opus) agents, one per batch of posts | the coordinator writing programs |
| PL-D19 | **the owner's:** nothing computes on the owner's Mac; every `abt`, build, test and data step on the machine | local runs |
| PL-D20 | **the owner's:** every window from the data's first bar (plus warm-up) to `H`, never a cap; the three-year guard removed | a window cap for memory |
| PL-D21 | **the owner's:** the analyst and the quant on `gpt-6.1-sol` (cutoff Apr 30, 2026; $2 / $0.10 / $10 per million input / cached / output), per-job caps $0.50 and $2 | `gpt-6-astra` (the top model, excluded by the owner's earlier rule) |
| PL-D22 | runs share the machine through `abt-slot`: five 10 GB slots, a data load reserves one, runs are killed before loads, checks and data queries skip the queue | unbounded runs (three machine-wide OOM kills) |
| PL-D23 | each data store on its own service with a resident budget and two glibc arenas; the served futures store is a hard-link snapshot behind the symlink `store-futures-current` | one service over every store |
| PL-D24 | **the owner's:** futures from Databento only; Norgate's continuous futures retired (the service disabled, the stores kept) | Norgate's decades of daily history |
| PL-D25 | every Databento load passes `--session-break 17:45 --session-gap 20`; each root's five bar relations consolidated and row-counted before the next | the engine's defaults before E-108; trusting the overlay after E-121 |
| PL-D26 | **the owner's:** Databento roots bought only when the free cost preview says $0 (OHLCV is in the subscription); saved to S3 under new prefixes only, with a sha256 manifest, verified, the write permission removed after | buying trades or order-book data |
| PL-D27 | the full equity store's service disabled until E-120 bounds a binding (it reached 45 GB in three minutes); equities read the S&P 500 store (2014 on) meanwhile | serving it unbounded |
| PL-D28 | the OpenAI guard counts every recorded job, the attic included (it read $21.90 when $35.28 had been spent) | counting only live jobs |
| PL-D29 | **the owner's:** the site at `https://backtests.signoscapital.com` behind Google sign-in for `@signoscapital.com` accounts. DNS stays at GoDaddy, so no Cloudflare (Tunnel and Access need the zone on Cloudflare): Caddy terminates TLS with Let's Encrypt, oauth2-proxy admits the Workspace domain (the OAuth app is Internal, so Google refuses other accounts first), the static nginx listens on 127.0.0.1 only. Supersedes PL-D6 and the IP-only port 80 | Cloudflare Tunnel and Access (the nameservers would move off GoDaddy); the IP allowlist (it broke with every new IP) |
| PL-D120 to PL-D131 | the report site's decisions, in `crates/abt-report/README.md` and its agent's commits | |

## Owner inputs (the default taken)

1. **The X API.** No bearer token; X through xAI (PL-D9) and the public mirror.
2. **The data's review.** The owner said the data is valid; every relation marked `reviewed = complete` (PL-D16).
3. **The models.** The owner moved the analyst and the quant to `gpt-6.1-sol` (PL-D21) after the Opus agents' night (PL-D18).
4. **The page's reach.** The owner chose Google sign-in for any `@signoscapital.com` account at `backtests.signoscapital.com`, DNS kept at GoDaddy (PL-D29).
5. **More Opus batches.** Every full backtest so far came from the Opus agents; whether to use them again beside `gpt-6.1-sol` is the owner's call.

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
| the full equity store's service | disabled (PL-D27) | ETFs and non-S&P stocks, equities before 2014; E-120's kernel side (a list-gated binding scoped to the lists' members) is in progress |
| the equity position-limit invariant | halts runs whose held position drifts above the 10 % clamp | six posts |
| N-hour bars (`@2h`) | no supported form | three posts |
| the held contract's own price | `contract()` reads nothing; an `adjustment(A, T, X)` relation is agreed, not built | levels, ratios and spreads on actual prices |
| intraday speed | about 1.2 ms a minute bar per root; a full-history intraday plan takes 10–15 h | intraday ideas on full history |
| the load's memory | each version rebuilds every instrument (E-89); consolidation per root keeps loads at 2–5 GB | an incremental version build |
| Databento roots | EMD (a server error mid-pull), MYM, M2K, MBT, MET, KE, SR3, LBR (pulls began before the listing) not yet fetched; 6C and ZQ keep few bars (missing definitions); the live extension, IFUS, TradeStation 2001+ not yet staged | ideas on those markets; futures before 2010 |
| the 16 full backtests | on Norgate futures (PL-D24) and before E-112/114/115 | a Databento rerun |

## Money spent today

| Service | Amount | Guard |
| --- | --- | --- |
| X, through xAI | **$5.55**: the probe, the fetch of the 50 posts, the mining of campaign 2 | $25, hard |
| OpenAI | **$35.28**, every recorded job (PL-D28): `gpt-6-luna` on Oct 19, `gpt-6.1-sol` from Oct 20 04:00 | $50, hard |
| Databento | **$0**: nine roots of OHLCV and definitions, covered by the subscription | the writer refuses any request above $0.01 |
| AWS | about **$12** to Oct 20 14:00 UTC: m7i 3.75 h, r7i 18 h, the volume 22 h | none set |

## The trace at the final tree

`absolute-backtest`'s `docs/spec/TRACE.md` on the amendments branch at e0aad8b: 202 rows, 154 done, 37 partial, 11 missing, 0 contradicts. The errata added overnight: E-104 (contracts synthesised from raw symbols), E-105 (session bars for daily programs), E-106 (a session window not yet begun), E-107 (`month_start` on a sessions clock), E-108 (CME session defaults), E-109 (the ladder's index once per level), E-110 (the services' memory, the schema list, derived relations without logs, pruning), E-111 (a version at a finer exponent), E-112 (`interval > 0`), E-113 (the Cboe series), E-114 (a window's trades), E-115 (each run compiled at its own point), E-116 (roll commission), E-117 (a root listed late), E-118 (`compare … by`), E-120 in part (`Distinct` by bucket), E-121 (rows lost when a relation was rebuilt after appends).

## The pull requests and the page

- Amendments: https://github.com/KurtCoVayne/absolute-backtest/pull/76
- The pipeline: https://github.com/KurtCoVayne/absolute-pipeline/pull/1
- The page: **https://backtests.signoscapital.com**, after Google sign-in with a `@signoscapital.com` account (PL-D29). Each post links to its page: the tweet, the idea and its assumptions, the analysis, the program and plan as run, the equity and drawdown charts with the held-out part shaded, the full / in-sample / out-of-sample table, contribution by market and year, the sensitivity grid, the red flags, the author's numbers beside ours, the verdict.
- Run the queued posts on `gpt-6.1-sol` against the served Databento snapshot: `scripts/aws/ssh.sh 'cd /srv/absolute/absolute-pipeline && FUTURES_STORE=/srv/absolute/store-futures-current bash deploy/run-sol-databento.sh'`. The OpenAI guard stops at $45 of every recorded job.

# abt-gateway

The only entry to the three backtester services for the pipeline (`docs/design/pipeline.md`, PL-7; `docs/design/contracts.md`, sections 5 and 6). Towards the sandboxes, a JSON tool API on `127.0.0.1:7777`, one route per capability of PL-6.4, every request carrying the job's Biscuit token; towards the layers, `abt-wire` frames over the Unix sockets of `abt-data`, the kernel pool and `abt-query`. Between the two, policy only: the clamp of every query to the job's horizon `H`, the plan guards, the per-job budgets, the fair-share queue in front of the kernels, the `pipeline.*` provenance, one audit row per request, and the forward scheduler.

```
abt-gateway serve   --data data.sock --kernels k1.sock,k2.sock,k3.sock,k4.sock --query query.sock \
                    --library corpus-v2/catalog --library corpus-v2/lib \
                    --listen 127.0.0.1:7777 --token-public-key /etc/absolute-pipeline/token_public_key
abt-gateway forward --every week [--once] [--out DIR] ...     # PIPELINE_BROKERS set: the outcomes topic
abt-gateway retire ID --reason TEXT ...
abt-gateway audit tail [-n N] [--job ID] ...
abt-gateway token keygen FILE | mint --key FILE --job ID --h ISO [--caps ...]
```

`--json` on every command; logs are JSON lines on stderr (`--log-text` for people); `--markdown` logs every table as Markdown; `GET /metrics` exports the queue's depth and the counters in Prometheus text.

## What it enforces

| Rule | Where |
| --- | --- |
| The token's `cap("<route>")`, its expiry against `time(now)` (nanoseconds, as the facts are), its `job`, `h`, `lineage`, `plan_runs`, `grid_points` | `token.rs` |
| `r_max := min(request, H)`, `as_of_seq := seq(H)` (the last sequence received by `H`, found by a clamped `scan` with `with_seq`, cached per relation and horizon) on every `query` | `query.rs`, `horizon.rs` |
| The plan guards: the hold-out (`trailing:…` with `window … .. H`, or `after H`), `budget { money 0 }`, the house presets or a one-field variant named for the field, the grid within `grid_points`, the plan executions within `plan_runs`, the program in the lineage, no agent reveal of a forward plan | `plan.rs`, `jobs.rs` |
| One plan execution in flight per job; permits of the kernel pool with a fair share among jobs; `busy` past the depth (60) | `queue.rs` |
| `submit`: the hashes name a plan the job ran, `trials_used` is the lineage's count, the forward plan checked and its hash stored, one per job | `api.rs` |
| A row of `pipeline.audit` per request; `pipeline.document` and `pipeline.hypothesis` on the orchestrator's `provenance`; `pipeline.submission` on `submit`; `pipeline.outcome` by the scheduler | `provenance.rs` |

The gateway ships the house presets (`house_presets.dsl`) and adds them to every program's workspace while the library it was started with defines neither, so programs naming `futures_house` or `equities_house` check against the pinned tree. Where the guideline's text does not parse on the pinned tree, the nearest the grammar takes stands until the amendment lands (see the file's header).

## Tests

Everything runs in one process through the channel transport: a data service over a synthetic market whose late rows are received after `H`, a pool of kernel services, a query service, and the gateway's layers over the three, with the tool API on a loopback port.

```
cargo test -p abt-gateway            # the unit tests, the routes, GP-4, the scheduler, the load rows
bash scripts/mutate.sh               # every gp4-*.patch must make its test fail
```

Against the amendments tree of the backtester (`holdout after TIME`), patch the git sources to its worktree:

```
cargo test -p abt-gateway --config 'patch."https://github.com/KurtCoVayne/absolute-backtest".abt-data.path="/path/to/absolute-backtest/crates/abt-data"' ...   # and abt-wire, abt-kernel, abt-query, abt-tests, abt
```

The Kafka client is behind the `kafka` feature (the deployment's image builds with it); without it the scheduler writes outcomes to `--out DIR`.

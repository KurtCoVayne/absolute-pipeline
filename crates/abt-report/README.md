# abt-report

The static report site of the pipeline (`docs/design/pipeline.md` §7.4, `docs/design/contracts.md` §8): every page is HTML a person reads without a server or JavaScript (a few lines of script drive the feed's filters), one stylesheet, no external asset. Nothing here trades or runs a backtest: the site shows what the log's records and the query layer's report tables say.

## Build

```
abt-report build --brokers 127.0.0.1:19092 --data /run/abt/data.sock --out /var/lib/absolute/site [--dlq] [--facts machine.json]
abt-report build --fixtures crates/abt-report/fixtures --out /tmp/site --facts crates/abt-report/fixtures/facts.json
abt-report serve --out /tmp/site --listen 127.0.0.1:8080
abt-report fixtures --out crates/abt-report/fixtures
```

`build` reads the topics `control`, `docs`, `triage`, `hypotheses`, `submissions`, `outcomes` from offset 0 (a fresh consumer group each time, nothing committed; `--dlq` adds the three dead-letter topics), keeps the latest version per document id and applies the tombstones, then reads `pipeline.document`, `pipeline.hypothesis`, `pipeline.submission`, `pipeline.outcome`, `pipeline.audit` and `query.<plan>.report.*` for every submission's plan and forward plan through the data socket with `scan`. `--fixtures DIR` replaces both: JSON arrays of records in the `json::render` shape (`docs.json` … `control.json`, `dlq.json`), `pipeline/<relation>.json` tables and `report/<plan hash>/<table>.json` tables. `PIPELINE_BROKERS` stands in for `--brokers`.

The machine serves the directory with nginx (`deploy/nginx.conf`, `deploy/Dockerfile.report`) on 8080 behind the SSH tunnel; `serve` is the same directory over `axum` for local reading.

## Pages

| Page | Path | Shows |
| --- | --- | --- |
| Front | `index.html` | counts, the top of the leaderboard |
| Feed | `feed.html` | documents newest first with triage verdict, score and flags; filters by account, instrument, verdict; links to the document, its hypotheses and strategies |
| Document | `documents/<id>.html` | the text with cited spans marked, segments, entities, provenance, triage, what it led to; a tombstoned document shows id, hashes and `source deleted` |
| Hypothesis | `hypotheses/<id>.html` | statement, `H`, evidence spans linked into the documents, data needs, author's numbers, claims, the submission or the reasoned failure |
| Strategy | `strategies/<id>.html` | the chain post → hypothesis → submission → reveals; the five-part conclusion; Full / in-sample / out-of-sample / forward; the author comparison; the red flags (guideline §12) with the ones that fire marked; contribution by market and year; the surface as a heat table; the forward track (window, metrics, verdict pairs, DSR, track-record progress); claims, contract, biases, trials, warnings, folds; the program, plan and forward plan texts |
| Leaderboard | `leaderboard.html` | forward DSR, then track-record length; the BH-adjusted q of the forward claim per calendar week of submission; in-sample-only submissions below |
| Accounts | `accounts.html`, `accounts/<handle>.html` | documents, hypotheses, submissions, forward hit rate, volume by week, cost |
| Operations | `operations.html` | budgets consumed against the `BudgetRow`s, queue lags from the audit, the dead-letter queues, tombstones, the follow list, the machine's facts |

Every page's footer carries the build time and how far each input was read.

## Tests

`cargo test -p abt-report`: the fixtures build the whole site and every page is checked (`tests/site.rs`); `tests/broker.rs` needs `PIPELINE_BROKERS` and skips with a reason without it. `cargo test -p abt-report --test site -- --ignored --nocapture projected` builds a thousand strategies and prints the time.

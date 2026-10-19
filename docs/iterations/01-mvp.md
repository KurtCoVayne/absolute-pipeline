# Iteration 1 · The MVP: the machine, the amendments, the pipeline end to end

Oct 19, 2026. The first iteration of `absolute-pipeline`, in the form of `absolute-backtest`'s `docs/iterations/01-foundation.md`: five phases per milestone (verify, fill gaps, test, performance, deploy) and this report at the end. Its scope is the whole MVP of `docs/design/pipeline.md` section 8 (MVP-0 to MVP-6), the amendments it asks of `absolute-backtest` (PL-8.1), and the three backtester programs deployed beside it on one machine.

## Where the tree starts

`absolute-pipeline` at d4b1e3d holds the documents only: the plan, the house rules, the seed fixtures, the colleague's framework. `absolute-backtest` is at a4128ca on `spec/three-layer-split` (iteration 1 landed: the three programs, `abt-codec`, the hot store). The owner's secrets as received: an OpenAI key and an xAI key; **no X API bearer token** (owner input 1 below).

## The machine

<!-- MACHINE -->

## Plan

1. The amendments pull request on `absolute-backtest` (PL-8.1), since the gateway needs them.
2. MVP-0 and MVP-1: `pipeline-wire`, `ingest-core`, `ingest-extract`, `ingest-x`, `ingest-fetch`, the `file` adapter, Redpanda, `ingest bench`.
3. MVP-2: the orchestrator, Biscuit tokens, the sandbox pool under gVisor, the egress proxy, the Python harness on the OpenAI Agents SDK, the nine skills, triage and analyst.
4. MVP-3: `abt-gateway`, the clamps, the plan guards, the kernel pool, `pipeline.*`, the forward scheduler; GP-4 in-process and through the tool API.
5. MVP-4: GP-5's usability task on `gpt-6-luna`; the quant; the seed campaign.
6. MVP-5: `abt-report`, the site.
7. MVP-6: the pools at capacity; GP-7.

## Report

<!-- REPORT -->

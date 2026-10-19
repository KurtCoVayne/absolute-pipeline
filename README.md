# absolute-pipeline

The system around [`absolute-backtest`](https://github.com/KurtCoVayne/absolute-backtest): a layer that consumes public content (X posts first; papers and feeds later) and formats it for agents, a queue-driven layer of sandboxed agents that turn content into strategies, and the backtester behind a gateway that clamps every query to a submission's information horizon. The product is a growing population of time-stamped, pre-registered strategies with forward evidence, each counted against its lineage's trials.

Nothing here trades. The pipeline ends at evidence and presentation.

## The documents

| Path | What it is |
| --- | --- |
| `docs/design/pipeline.md` | The design plan (Oct 19, 2026): principles, the shared records and the log, the ingest layer (sources, the X account follower, extraction, enrichment, guardrails, the hot path's budgets), the agents (runtime, definitions, skills, capabilities, stages, honest evaluation, sandboxing, budgets), the gateway and the forward scheduler, presentation, the MVP with gates GP-1 to GP-7, what the owner decided and what stays open |
| `docs/design/agent-guidelines.md` | The house rules every agent loads (`agents-skills/skills/house-rules/SKILL.md` will be this file verbatim): what we test on, fixed capital with no compounding, sizing, costs, no lookahead, no survivorship, adjusted series, faithfulness to the author, the quant protocol, the two hold-outs, the default claims, the red flags, the conclusion's form, the house presets and plan templates; each rule marked enforced, checked or instructed |
| `docs/design/fixtures/follow-list.toml` | The seed follow list: 37 accounts, tier 1 for the 12 shared twice or more |
| `docs/design/fixtures/seed-posts.tsv` | The 50 seed posts with their creation times, the first campaign and GP-5's corpus |
| `docs/references/quant-idea-backtest/` | The colleague's futures framework as received (its `SKILL.md`, `conventions.md`, `protocol.md`, `README.md`): the source of the house rules, kept verbatim for reference; its engine and data are not used here |

The specification of the backtester itself (`docs/spec/`, the language, the formal foundations, the studies) lives in `absolute-backtest`; the plan cites it by its section ids (`A-`, `D-`, `K-`, `Q-`, `P-`, `M-`, `ST-`, `E-`).

## Layout to come

As `docs/design/pipeline.md` section 3 lays it out: a Cargo workspace with `pipeline-wire`, `ingest-core`, `ingest-x`, `ingest-extract`, `ingest-fetch`, `agents-orchestrator`; `agents-skills/` (definitions in TOML, skills in Markdown); `agents-harness/` (Python, the OpenAI Agents SDK, run inside the sandbox); a Compose file for the one machine. `abt-gateway` and `abt-report` live in `absolute-backtest`, beside the three programs they front.

## Decisions that shape it

- The information horizon `H` is explicit on every hypothesis and submission; the gateway clamps every query to it; the forward window after it is revealed by a scheduler, never by an agent (plan §4.2, §6.6).
- Content is data, never instruction; what an agent may do is its capability token, not its prompt (plan §5.5, §6.4).
- Kafka protocol on Redpanda; FlatBuffers records with canonical-JSON ids; Biscuit tokens; one sandbox per job; the small OpenAI model everywhere unless a test forces the quant to the mid one; one 32 GB AWS machine; the owner's Databento store as the only data; no slippage for starters; English everywhere (plan §11).

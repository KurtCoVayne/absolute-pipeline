# agents-harness

The agents' harness (`docs/design/pipeline.md` PL-6.1, `docs/design/contracts.md` section 3): `python -m agents_harness` runs one job inside its sandbox on the OpenAI Agents SDK (`openai-agents` 0.24.0, the Responses API), with exactly the tools the job's capability token grants, under the job's budget, and writes `/job/out`.

```
agents_harness/
  __main__.py   the entrypoint: load /job/in, run the stage, write /job/out, exit 0 | 2 (budget) | 3 (internal)
  job.py        the job directory (job.json, token, definition.toml, skills/, docs/, hypothesis.json)
  skills.py     definitions and skills, their hashes (contracts section 7), PL-6.4's closed set of caps
  envelope.py   the <document … untrusted> envelope and the injection scanner
  runner.py     the model client through the proxy (OPENAI_BASE_URL, X-Pipeline-Job), the prompt, the budget hooks, the input guardrail
  tools.py      one tool set per granted capability: the gateway's JSON API, read_document, note; the quant's protocol state
  guards.py     the checked rules read from texts: house presets, the two hold-outs, after claims, the translation check, the comparison table
  budget.py     tokens, tool calls, wall time, money at the rate card (PL-8.5; PIPELINE_RATES overrides)
  gateway.py    POST <ABT_GATEWAY>/v1/<route> with Authorization: Biscuit …, retried on 429 and 5xx
  records.py    the structured outputs (Pydantic) and the records in json::render's shape
  stages.py     triage, analyze, strategize
usability/      GP-5's task: R8L from the docs on the chosen model (see below)
tests/          a fake OpenAI server and a fake gateway; the live smoke tests
```

## Run the tests

```
cd agents-harness
uv venv --python 3.12 .venv && uv pip install --python .venv/bin/python -r requirements.txt pytest ruff
.venv/bin/ruff check . && .venv/bin/ruff format --check .
.venv/bin/python -m pytest -m "not live"
```

Everything runs against the fake servers in `tests/`; nothing reaches the network.

## The live smoke tests

One per agent on `gpt-6-luna` through the real API, each capped under $0.10 by the job's budget; they run only when `SECRETS/openai_api_key` exists (the test reads it itself; it is never printed):

```
.venv/bin/python -m pytest -m live -s tests/test_live.py
```

The gateway is the fake one, so the quant's smoke test proves the loop runs under the harness's guards and ends with a status, not that a submission is made.

## Run a job by hand

```
JOB_DIR=/path/to/job OPENAI_BASE_URL=http://10.200.0.1:8080/openai/v1 OPENAI_API_KEY=sandbox ABT_GATEWAY=http://10.200.0.1:8080/gateway \
  .venv/bin/python -m agents_harness
```

`/job/in` as contracts section 3; the skills are read from `/job/in/skills/<name>/SKILL.md`, falling back to `/opt/skills` in the image.

## The usability task (GP-5)

`usability/` runs the R8L task of `docs/llm-usability.md` through the same harness: the `abt-language` and `abt-idioms` skills, a `check` tool only, the task text of `usability/r8l.md`. The integrator provides the gateway:

```
ABT_GATEWAY=http://127.0.0.1:7777 ABT_TOKEN_FILE=/path/to/token usability/run.sh gpt-6-luna      # runs; writes usability/runs/<model>-<time>/
usability/run.sh gpt-6-luna --dry-run                                                              # prints the prompt, calls nothing
.venv/bin/python usability/score.py usability/runs/<dir> [--run-decisions N]                       # attempts, first clean check, whether it trades
```

The scorer reads the run's `attempts/*.dsl` and `check/*.json`; whether the final program trades comes from the integrator's `abt run` of `final.dsl` on the R8L bundle (`--run-decisions N`, or a `run.json` with `decisions` beside the attempts).

"""GP-5's usability task: write R8L from the docs on the chosen model, through
the same harness, with the `abt-language` and `abt-idioms` skills and a
`check` tool only (plus `note`). The integrator provides the gateway.

    ABT_GATEWAY=http://127.0.0.1:7777 ABT_TOKEN_FILE=/path/to/token python usability/run.py --model gpt-6-luna
    python usability/run.py --model gpt-6-luna --dry-run        # prints the prompt, calls nothing

Writes usability/runs/<model>-<time>/: job/ (what the harness read), attempts/N.dsl
(every program passed to `check`, in order), check/N.json (its diagnostics),
final.dsl, notes.md, cost.json, result.json. `score.py` reads that directory.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import shutil
import sys
from pathlib import Path

from pydantic import BaseModel, Field

HERE = Path(__file__).resolve().parent
HARNESS = HERE.parent
REPO = HARNESS.parent
sys.path.insert(0, str(HARNESS))

from agents_harness.budget import BudgetExceeded, Meter, load_rates, rate_for  # noqa: E402
from agents_harness.gateway import Gateway  # noqa: E402
from agents_harness.job import load_inputs  # noqa: E402
from agents_harness.output import write_cost, write_notes  # noqa: E402
from agents_harness.runner import ModelUnavailable, build_agent, make_client, prompt_hash, render_instructions, run_agent  # noqa: E402
from agents_harness.skills import skills_hash  # noqa: E402
from agents_harness.tools import Context, ProtocolEnd  # noqa: E402

TASK = HERE / "r8l.md"
H = "2026-08-14T21:00:00Z"  # the task's data ends on 2026-08-14; the horizon only labels the prompt here


class UsabilityOut(BaseModel):
    program: str = Field(description="final.dsl: the complete program, byte for byte the text of the last clean check")
    checks_clean: bool = Field(description="whether the final program checked clean")
    notes: str = Field(description="NOTES.md: what was hard, what the docs lacked, which constructs were guessed, which rules are unsure, under 400 words")


def build_job_dir(run_dir: Path, model: str, token: str) -> Path:
    import tomllib

    job_dir = run_dir / "job"
    inp = job_dir / "in"
    (inp / "docs").mkdir(parents=True)
    (job_dir / "out").mkdir()
    definition = (HERE / "definition.toml").read_text()
    definition = definition.replace('id = "gpt-6-luna"', f'id = "{model}"')
    (inp / "definition.toml").write_text(definition)
    d = tomllib.loads(definition)
    for name in d["agent"]["skills"]:
        dst = inp / "skills" / name
        dst.mkdir(parents=True)
        shutil.copy(REPO / "agents-skills" / "skills" / name / "SKILL.md", dst / "SKILL.md")
    job = {
        "record": "Job",
        "id": f"usability-{model}-{dt.datetime.now(dt.UTC).strftime('%Y%m%dT%H%M%SZ')}",
        "stage": "usability",
        "inputs": [],
        "agent": "usability",
        "definition_hash": "",
        "skills_hash": "",
        "model": model,
        "caps": d["agent"]["caps"],
        "budget": d["budget"],
        "attempt": 1,
        "state": "Running",
        "h": H,
        "knowledge_cutoff": d["agent"]["model"]["knowledge_cutoff"],
        "lineage": "",
        "campaign": "usability",
    }
    (inp / "job.json").write_text(json.dumps(job, indent=1))
    (inp / "token").write_text(token + "\n")
    (inp / "docs" / "closure.json").write_text(json.dumps({"inputs": [], "closure": []}))
    return job_dir


def user_prompt() -> str:
    task = TASK.read_text()
    return (
        "You are writing a trading strategy in abt, the language the skills describe. Your only tools are `check(program, name)` "
        "(the checker: read its diagnostics; a clean check has no error) and `note(text)`. There is no run, no data query and no file system: "
        "the checker is your only feedback, so read the skills' idioms closely and review your program before each check.\n\n"
        "Rules: implement the task below faithfully as a strategy named `r8l`; save nothing elsewhere; call `check` on every version "
        "(at most about 30 checks); stop when the program checks clean and you believe it implements the specification; "
        "then answer with the final program, whether it checked clean, and your notes (what was hard, what the docs lacked or got wrong, "
        "which constructs you guessed at, which rules you are unsure you implemented faithfully).\n\n"
        "The execution block is `execution futures_house` (the house preset; the task's run conventions are what it encodes: fixed base, "
        'fractional contracts, commission per contract, no slippage, no financing, unconstrained leverage). The reporting calendar is `sessions of "ES"`.\n\n'
        "# TASK.md\n\n" + task
    )


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--model", default="gpt-6-luna")
    ap.add_argument("--dry-run", action="store_true", help="print the prompt and exit; nothing is called")
    ap.add_argument("--out", default=str(HERE / "runs"), help="where the run directory goes")
    ap.add_argument("--gateway", default=os.environ.get("ABT_GATEWAY"), help="the gateway's base URL (ABT_GATEWAY)")
    ap.add_argument("--token-file", default=os.environ.get("ABT_TOKEN_FILE"), help="a file holding the job's Biscuit token (ABT_TOKEN_FILE)")
    args = ap.parse_args(argv)

    stamp = dt.datetime.now(dt.UTC).strftime("%Y%m%dT%H%M%SZ")
    run_dir = Path(args.out) / f"{args.model}-{stamp}"
    token = ""
    if not args.dry_run:
        if not args.gateway or not args.token_file:
            print("usability: ABT_GATEWAY and ABT_TOKEN_FILE (or --gateway and --token-file) are needed to run; --dry-run prints the prompt", file=sys.stderr)
            return 2
        token = Path(args.token_file).read_text().strip()
        os.environ["ABT_GATEWAY"] = args.gateway
    run_dir.mkdir(parents=True, exist_ok=True)
    job_dir = build_job_dir(run_dir, args.model, token or "dry-run")
    inputs = load_inputs(job_dir)
    instructions = render_instructions(inputs.definition, inputs.skills, inputs, H)
    prompt = user_prompt()
    if args.dry_run:
        print("=== system instructions ===")
        print(instructions)
        print("=== user prompt ===")
        print(prompt)
        print(f"=== prompt hash {prompt_hash(instructions)}; run directory {run_dir} ===")
        return 0

    meter = Meter(budget=inputs.job.budget, model=args.model, rate=rate_for(args.model, load_rates()))
    gateway = Gateway(token=token, deadline=lambda: meter.remaining_s)
    ctx = Context(inputs=inputs, meter=meter, gateway=gateway, skills_hash=skills_hash([(s.name, s.raw) for s in inputs.skills]))
    ctx.prompt_hash = prompt_hash(instructions)
    client = make_client(inputs.job.id)
    agent = build_agent("usability", instructions, ctx, UsabilityOut, model_id=args.model, client=client)
    result: dict[str, object] = {"model": args.model, "run_dir": str(run_dir), "prompt_hash": ctx.prompt_hash}
    final: UsabilityOut | None = None
    try:
        run = run_agent(agent, prompt, ctx, max_turns=int(inputs.job.budget.tool_calls) + 2)
        final = run.final_output_as(UsabilityOut)
        result["status"] = "done"
    except BudgetExceeded as e:
        result["status"] = f"budget: {e}"
    except ProtocolEnd as e:
        result["status"] = f"{e.reason}: {e.detail}"
    except ModelUnavailable as e:
        result["status"] = f"model_unavailable: {e}"
    finally:
        gateway.close()
        save_attempts(run_dir, gateway)
        if final is not None:
            (run_dir / "final.dsl").write_text(final.program)
            ctx.notes.append("## NOTES.md\n" + final.notes)
            result["checks_clean_claimed"] = final.checks_clean
        write_notes(run_dir, ctx.notes)
        write_cost(run_dir, meter.cost())
        result["cost"] = meter.cost()
        result["checks"] = ctx.quant.checks_used
        (run_dir / "result.json").write_text(json.dumps(result, indent=1) + "\n")
    print(json.dumps(result, indent=1))
    return 0 if result.get("status") == "done" else 1


def save_attempts(run_dir: Path, gateway: Gateway) -> None:
    (run_dir / "attempts").mkdir(exist_ok=True)
    (run_dir / "check").mkdir(exist_ok=True)
    n = 0
    for (route, body), answer in zip(gateway.calls, gateway.answers, strict=False):
        if route != "check":
            continue
        n += 1
        (run_dir / "attempts" / f"{n}.dsl").write_text(str(body.get("program", "")))
        (run_dir / "check" / f"{n}.json").write_text(json.dumps(answer, indent=1) + "\n")


if __name__ == "__main__":
    sys.exit(main())

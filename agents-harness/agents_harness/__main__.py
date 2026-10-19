"""`python -m agents_harness`: run the job at `$JOB_DIR` (contracts section 3).

Exit 0 when `status.json` is written (done or a reasoned failure), 2 on a
budget overrun (`status.json` says `budget`), 3 on an internal error.
"""

from __future__ import annotations

import os
import sys
import traceback
from pathlib import Path

from .budget import BudgetExceeded, Meter, load_rates, rate_for
from .gateway import Gateway
from .guards import h_text
from .job import JobError, job_dir_from_env, load_inputs
from .local import LocalAbt
from .output import write_cost, write_notes, write_records, write_status
from .runner import InvalidOutput, ModelUnavailable, make_client
from .skills import DefinitionError, SkillError, skills_hash
from .stages import STAGES
from .tools import Context

EXIT_OK = 0
EXIT_BUDGET = 2
EXIT_INTERNAL = 3


def main(argv: list[str] | None = None) -> int:
    job_dir = job_dir_from_env()
    if argv:
        job_dir = Path(argv[0])
    out_dir = job_dir / "out"
    try:
        inputs = load_inputs(job_dir)
    except (JobError, DefinitionError, SkillError, OSError, ValueError) as e:
        write_status(out_dir, "failed", "invalid_input", False, str(e))
        print(f"agents_harness: invalid input: {e}", file=sys.stderr)
        return EXIT_INTERNAL

    job = inputs.job
    model_id = job.model or inputs.definition.model.id
    meter = Meter(budget=job.budget, model=model_id, rate=rate_for(model_id, load_rates()))
    gateway: Gateway | LocalAbt | None
    if os.environ.get("ABT_LOCAL") == "1" and any(c.startswith("abt.") for c in job.caps):
        # The owner's cut of Oct 19: the tools answered by `abt` on the machine itself.
        gateway = LocalAbt(h=h_text(job.h), work=out_dir / "abt")
    elif os.environ.get("ABT_GATEWAY") or any(c.startswith("abt.") for c in job.caps):
        gateway = Gateway(token=inputs.token, deadline=lambda: meter.remaining_s)
    else:
        gateway = None
    ctx = Context(inputs=inputs, meter=meter, gateway=gateway, skills_hash=skills_hash([(s.name, s.raw) for s in inputs.skills]))

    stage = STAGES.get(job.stage)
    if stage is None:
        write_status(out_dir, "failed", "invalid_input", False, f"unknown stage {job.stage!r}")
        return EXIT_INTERNAL

    code = EXIT_OK
    try:
        client = make_client(job.id)
        outcome = stage(ctx, model_id=model_id, client=client)
        write_records(out_dir, outcome.records)
        write_status(out_dir, outcome.state, outcome.reason, outcome.retryable, outcome.detail)
    except BudgetExceeded as e:
        write_records(out_dir, [])
        write_status(out_dir, "failed", "budget", False, str(e))
        code = EXIT_BUDGET
    except InvalidOutput as e:
        write_records(out_dir, [])
        write_status(out_dir, "failed", "invalid_output", False, str(e))
    except ModelUnavailable as e:
        write_records(out_dir, [])
        write_status(out_dir, "failed", "model_unavailable", True, str(e))
    except Exception as e:  # noqa: BLE001 - the sandbox's last line of defence
        write_records(out_dir, [])
        write_status(out_dir, "failed", "internal", True, f"{type(e).__name__}: {e}")
        traceback.print_exc()
        code = EXIT_INTERNAL
    finally:
        write_notes(out_dir, ctx.notes)
        write_cost(out_dir, meter.cost())
        if gateway is not None:
            gateway.close()
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

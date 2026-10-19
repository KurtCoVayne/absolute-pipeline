"""Score a usability run directory (what `run.py` wrote): the attempts, the
first clean check, and whether the final program trades.

    python usability/score.py usability/runs/<dir> [--run-decisions N]

Whether the program trades comes from the integrator's `abt run` of `final.dsl`
on the R8L bundle: pass its decision count as `--run-decisions`, or put a
`run.json` with {"decisions": N, ...} in the run directory. Writes score.json
and prints one table (the columns of docs/llm-usability.md's results table).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def score(run_dir: Path, run_decisions: int | None = None) -> dict[str, object]:
    attempts = sorted((run_dir / "attempts").glob("*.dsl"), key=lambda p: int(p.stem)) if (run_dir / "attempts").is_dir() else []
    rows = []
    first_clean: int | None = None
    for p in attempts:
        n = int(p.stem)
        check_file = run_dir / "check" / f"{n}.json"
        answer = json.loads(check_file.read_text()) if check_file.is_file() else {}
        diags = (
            answer.get("diagnostics", [])
            if isinstance(answer, dict) and "diagnostics" in answer
            else (answer.get("error", {}) and [{"code": answer["error"].get("code"), "severity": "error", "message": answer["error"].get("message")}])
        )
        diags = diags or []
        errors = [d for d in diags if str(d.get("severity", "")).lower() == "error"]
        warnings = [d for d in diags if str(d.get("severity", "")).lower() == "warning"]
        clean = not errors and bool(answer.get("program_hash")) if isinstance(answer, dict) else False
        if clean and first_clean is None:
            first_clean = n
        rows.append({"attempt": n, "errors": sorted({str(d.get("code")) for d in errors}), "warnings": sorted({str(d.get("code")) for d in warnings}), "clean": clean, "bytes": p.stat().st_size})
    final = run_dir / "final.dsl"
    final_matches_clean = False
    if final.is_file() and attempts:
        text = final.read_text()
        final_matches_clean = any(r["clean"] and attempts[i].read_text() == text for i, r in enumerate(rows))
    if run_decisions is None and (run_dir / "run.json").is_file():
        run_decisions = int(json.loads((run_dir / "run.json").read_text()).get("decisions", 0))
    result = json.loads((run_dir / "result.json").read_text()) if (run_dir / "result.json").is_file() else {}
    return {
        "run_dir": str(run_dir),
        "model": result.get("model"),
        "status": result.get("status"),
        "attempts": len(attempts),
        "first_clean_check": first_clean,
        "final_present": final.is_file(),
        "final_is_a_clean_attempt": final_matches_clean,
        "trades": None if run_decisions is None else run_decisions > 0,
        "decisions": run_decisions,
        "cost": result.get("cost"),
        "per_attempt": rows,
    }


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("run_dir")
    ap.add_argument("--run-decisions", type=int, default=None, help="the decision count of `abt run` on final.dsl over the R8L bundle")
    args = ap.parse_args(argv)
    run_dir = Path(args.run_dir)
    s = score(run_dir, args.run_decisions)
    (run_dir / "score.json").write_text(json.dumps(s, indent=1) + "\n")
    trades = "unknown (run final.dsl and pass --run-decisions)" if s["trades"] is None else ("yes" if s["trades"] else "no (decisions = 0: a failure)")
    print("| run | attempts | first clean check | final is a clean attempt | trades | cost USD |")
    print("| --- | ---: | ---: | --- | --- | ---: |")
    cost = (s.get("cost") or {}).get("money_usd")
    print(f"| R8L · {s['model']} | {s['attempts']} | {s['first_clean_check'] or '—'} | {'yes' if s['final_is_a_clean_attempt'] else 'no'} | {trades} | {cost if cost is not None else '—'} |")
    for r in s["per_attempt"]:
        print(f"  attempt {r['attempt']}: {'clean' if r['clean'] else 'errors ' + ','.join(r['errors'])}{' warnings ' + ','.join(r['warnings']) if r['warnings'] else ''}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

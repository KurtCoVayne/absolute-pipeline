"""The local backend's failure text, its memory rule and query_run on a job's run."""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Any

from agents_harness import local
from agents_harness.local import LocalAbt, available_gb, failure_text


def done(code: int, stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess[str]:
    return subprocess.CompletedProcess(["abt"], code, stdout, stderr)


def test_a_killed_plan_says_so_and_stderr_is_kept_once() -> None:
    killed = failure_text(done(-9), "abt plan run")
    assert "killed by signal 9" in killed and "out-of-memory" in killed and "Narrow the window" in killed
    err = failure_text(done(1, stderr="error: X\nwarning [W9] noise\nerror: X\nerror: Y\n"), "abt plan run")
    assert err.startswith("abt plan run failed (exit 1).") and err.count("error: X") == 1 and "error: Y" in err and "W9" not in err
    assert len(failure_text(done(1, stderr="e" * 5000), "x")) < 2_100


def test_available_memory_is_read() -> None:
    assert available_gb() > 0


def test_query_run_reruns_the_runs_program_at_its_point_over_its_window(tmp_path: Path, monkeypatch: Any) -> None:
    calls: list[list[str]] = []

    def fake_abt(self: LocalAbt, args: list[str], timeout: float = 0) -> subprocess.CompletedProcess[str]:
        calls.append(args)
        return done(0, stdout="warning [W9] x\n? coverage over 2026-01-02 .. 2026-03-01\nrelation  calls  tuples\nentry  120  0\n")

    monkeypatch.setattr(LocalAbt, "_abt", fake_abt)
    monkeypatch.setattr(local.glob, "glob", lambda pat: [])
    b = LocalAbt(h="2026-09-19T17:55:02Z", work=tmp_path)
    program = "strategy ibs_moo {\n}\n"
    plan = "study idea_ibs {\n  window 2023-09-20 .. 2026-09-19T17:55:02Z\n  steps { run base   run sanity over 2025-01-02 .. 2026-09-19   grid sens   reveal oos of base }\n}\n"
    b.plans["h1"] = {
        "plan": plan,
        "programs": {"ibs_moo.dsl": program},
        "runs": [{"step": "base", "run_id": "r-base", "point": [["n", "5"], ["risk", "7500 USD"]]}, {"step": "sanity", "run_id": "r-san", "point": []}],
    }
    out = b.call("query_run", {"run_id": "r-base", "text": "coverage over 2026-01-02 .. 2026-03-01 ;; why not decide(T, _) at 2026-02-02"})
    args = calls[-1]
    assert args[:5] == ["query", "--strategy", "ibs_moo", "--data", b.data]
    assert args[args.index("--start") + 1] == "2023-09-20" and args[args.index("--end") + 1] == "2026-09-19"
    assert "n=5" in args and "risk=7500 USD" in args
    assert args.count("--text") == 2 and "why not decide(T, _) at 2026-02-02" in args
    assert "W9" not in out["text"] and "entry  120  0" in out["text"]
    b.call("query_run", {"run_id": "r-san", "text": "trace decide at 2026-02-02"})
    assert calls[-1][calls[-1].index("--start") + 1] == "2025-01-02"


def test_no_window_cap_and_futures_read_databento_only(tmp_path: Path, monkeypatch: Any) -> None:
    from agents_harness.gateway import GatewayError
    from agents_harness.local import reads_minutes

    daily = "strategy d_moo {\n  data { class future  need bars @1m from 2010-06-07 complete }\n  calendars { session = sessions }\n  clock { decide on session }\n}\n"
    intraday = "strategy i {\n  data { class future  need bars @1m complete }\n  clock { decide on base at session + 30min }\n}\n"
    equity = "strategy e {\n  data { class equity  need bars @1d }\n  clock { decide on base }\n}\n"
    assert not reads_minutes(daily) and reads_minutes(intraday) and not reads_minutes(equity)
    assert reads_minutes("strategy f {\n  data { class future }\n  clock { decide on base }\n}\n")
    monkeypatch.setattr(local.glob, "glob", lambda pat: [])
    b = LocalAbt(h="2026-09-19T17:55:02Z", work=tmp_path)
    plan = "study idea_d {\n  window 2010-06-07 .. 2026-09-19T17:55:02Z\n  holdout trailing:20%\n  budget { money 0; bytes 2G; requests 0 }\n}\n"
    # The owner's rule: as much history as the store holds, daily or intraday.
    assert b.guard_plan(plan, {"d_moo.dsl": daily}) == plan
    assert b.guard_plan(plan, {"i.dsl": intraday}) == plan
    # Futures are Databento only (the owner, Oct 20): a Norgate root routes nowhere else unless ABT_NORGATE_FUTURES=1.
    monkeypatch.delenv("ABT_NORGATE_FUTURES", raising=False)
    assert b._data_for({"d_moo.dsl": "strategy d_moo { universe { instruments { ES_N from 1997-09-09 } } }"}) == b.data
    assert b.daily_roots() == []
    assert b._data_for({"d_moo.dsl": daily}) == b.data
    assert b._data_for({"e.dsl": equity}) == b.equity_data

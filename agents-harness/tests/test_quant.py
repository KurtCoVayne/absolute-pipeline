"""The quant's loop over the fake gateway: a submission after the protocol, and
the refusals of the checked rules (guideline sections 9, 11, 14, 15)."""

from __future__ import annotations

import json
from pathlib import Path

from .conftest import CANDIDATE_TEXT, H, build_job, doc
from .fake_openai import Message, ToolCalls
from .quant_fixtures import CHECK_PLAN, DEV_PLAN, FORWARD_PLAN, PROGRAM, RATIONALE, TRANSLATION_NOTE, hypothesis_record

PROG = {"program_name": "sma50_es", "program": PROGRAM}


def quant_job(tmp_path: Path):
    d = doc("post-1", CANDIDATE_TEXT)
    return build_job(tmp_path, "strategize", [d], hypothesis=hypothesis_record(d["id"]))


def done(outcome: str, detail: str = "") -> Message:
    return Message(json.dumps({"outcome": outcome, "detail": detail}))


def test_full_protocol_ends_in_a_submission(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = quant_job(tmp_path)
    fake_openai.script = [
        ToolCalls([("note", {"text": "Spec: 50-day SMA cross on ES, next open, no stop stated."}), ("schema", {"class_name": "future"}), ("resolve_symbols", {"symbols": ["ES"]})]),
        ToolCalls([("check", {"program": PROGRAM, "name": "sma50_es"})]),
        ToolCalls([("plan_run", {"plan": CHECK_PLAN, **PROG})]),
        ToolCalls([("query_run", {"run_id": "run_001", "text": "ledger fill"}), ("query_run", {"run_id": "run_001", "text": '? atr(A, T, V) for A = "ES" over 2019-01 .. 2019-06'})]),
        ToolCalls([("note", {"text": TRANSLATION_NOTE})]),
        ToolCalls([("plan_check", {"plan": DEV_PLAN, **PROG})]),
        ToolCalls([("plan_run", {"plan": DEV_PLAN, **PROG})]),
        ToolCalls([("plan_report", {"plan_hash": "FILLED"})]),
        ToolCalls([("plan_check", {"plan": FORWARD_PLAN, **PROG})]),
        ToolCalls([("submit", {"plan_hash": "FILLED", "forward_plan_text": FORWARD_PLAN, "rationale": RATIONALE})]),
        done("submitted", "sma50_es submitted"),
    ]
    # The plan hash is only known after plan_run: the fake fills it in from the gateway's answer.
    _fill_plan_hash(fake_openai, fake_gateway, DEV_PLAN)

    assert run_harness(built) == 0
    assert built.status()["state"] == "done", built.status()
    [record] = built.records()
    assert record["record"] == "Submission"
    assert record["id"] == "" and record["r"] == 0
    assert record["hypothesis_id"] == hypothesis_record(built.docs[0]["id"])["id"]
    assert record["h"] == H
    assert record["program_text"] == PROGRAM and record["plan_text"] == DEV_PLAN and record["forward_plan_text"] == FORWARD_PLAN
    assert len(record["program_hash"]) == 64 and len(record["plan_hash"]) == 64 and len(record["forward_plan_hash"]) == 64
    assert record["lineage"] == record["program_hash"]
    assert record["claims"] == [
        "survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after",
        "not_overfit    : pbo on grid sens expect statistic < 0.5 after",
    ]
    assert json.loads(record["insample"])["decisions"] == {"sanity": 37, "base": 37, "long_only": 37}
    assert record["trials_used"] == 34
    assert record["rationale"] == RATIONALE
    assert record["agent"] == "quant" and record["model"] == "gpt-6-luna" and len(record["skills_hash"]) == 64
    assert record["campaign"] == "test"
    routes = fake_gateway.routes_called()
    assert routes == ["schema", "resolve_symbols", "check", "plan_run", "query_run", "query_run", "plan_check", "plan_run", "plan_report", "plan_check", "submit"]
    submitted = fake_gateway.submitted
    assert submitted["program_text"] == PROGRAM and submitted["forward_plan_text"] == FORWARD_PLAN
    assert submitted["insample"]["metrics"]["columns"][0] == "step"
    assert every_request_carried_the_token(fake_gateway, built)
    notes = built.notes()
    assert "## Translation check" in notes
    assert built.cost()["tool_calls"] == 13


def test_submit_is_refused_without_the_translation_check(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = quant_job(tmp_path)
    fake_openai.script = [
        ToolCalls([("check", {"program": PROGRAM, "name": "sma50_es"})]),
        ToolCalls([("plan_run", {"plan": DEV_PLAN, **PROG})]),
        ToolCalls([("plan_report", {"plan_hash": "FILLED"})]),
        ToolCalls([("query_run", {"run_id": "run_002", "text": "ledger fill"})]),
        ToolCalls([("submit", {"plan_hash": "FILLED", "forward_plan_text": FORWARD_PLAN, "rationale": RATIONALE})]),
        done("could_not_check", "the harness refused the submission"),
    ]
    _fill_plan_hash(fake_openai, fake_gateway, DEV_PLAN)
    assert run_harness(built) == 0
    status = built.status()
    assert status["state"] == "failed" and status["reason"] == "could_not_check"
    assert built.records() == []
    assert "submit" not in fake_gateway.routes_called()
    refusal = fake_openai.requests[-1].input_text
    assert '"code":"translation_check"' in refusal and "Translation check" in refusal


def test_submit_is_refused_without_the_comparison_table(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = quant_job(tmp_path)
    bare = RATIONALE.split("| Measure | Author")[0] + "\n5. Verdict. Survives."
    fake_openai.script = [
        ToolCalls([("check", {"program": PROGRAM, "name": "sma50_es"})]),
        ToolCalls([("plan_run", {"plan": CHECK_PLAN, **PROG})]),
        ToolCalls([("query_run", {"run_id": "run_001", "text": "ledger fill"}), ("note", {"text": TRANSLATION_NOTE})]),
        ToolCalls([("plan_run", {"plan": DEV_PLAN, **PROG})]),
        ToolCalls([("plan_report", {"plan_hash": "FILLED"})]),
        ToolCalls([("submit", {"plan_hash": "FILLED", "forward_plan_text": FORWARD_PLAN, "rationale": bare})]),
        done("could_not_check", "refused"),
    ]
    _fill_plan_hash(fake_openai, fake_gateway, DEV_PLAN)
    assert run_harness(built) == 0
    assert "submit" not in fake_gateway.routes_called()
    assert '"code":"comparison"' in fake_openai.requests[-1].input_text


def test_plans_without_after_claims_or_off_the_hold_outs_are_refused(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = quant_job(tmp_path)
    no_claims = DEV_PLAN.replace("survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after", "").replace(
        "not_overfit    : pbo on grid sens expect statistic < 0.5 after", ""
    )
    no_holdout = DEV_PLAN.replace("holdout    trailing:20%", "holdout    none")
    bad_window = DEV_PLAN.replace(f"window     2000-01-03 .. {H.split('.')[0]}Z", "window     2000-01-03 .. 2026-10-01")
    forward_with_reveal = FORWARD_PLAN.replace("steps      { run base   report main }", "steps      { run base   reveal oos of base   report main }")
    fake_openai.script = [
        ToolCalls([("check", {"program": PROGRAM, "name": "sma50_es"})]),
        ToolCalls([("plan_run", {"plan": no_claims, **PROG})]),
        ToolCalls([("plan_run", {"plan": no_holdout, **PROG})]),
        ToolCalls([("plan_run", {"plan": bad_window, **PROG})]),
        ToolCalls([("plan_check", {"plan": forward_with_reveal, **PROG})]),
        ToolCalls([("plan_run", {"plan": DEV_PLAN, "program_name": "sma50_es", "program": PROGRAM + "\n# edited after the check\n"})]),
        done("could_not_check", "every plan was refused"),
    ]
    assert run_harness(built) == 0
    assert fake_gateway.routes_called() == ["check"]
    answers = [r.input_text for r in fake_openai.requests[2:]]
    assert '"code":"claims"' in answers[0]
    assert '"code":"holdout"' in answers[1] and "not admitted" in answers[1]
    assert '"code":"holdout"' in answers[2] and "ends at H" in answers[2]
    assert '"code":"reveal"' in answers[3]
    assert '"code":"unchecked"' in answers[4]


def test_execution_block_off_the_presets_is_refused_before_the_gateway(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = quant_job(tmp_path)
    inline = PROGRAM.replace("execution futures_house", "execution { price close orders moo lot fractional }")
    foreign = PROGRAM.replace("execution futures_house", "execution equity_default")
    two_fields = PROGRAM.replace("execution futures_house", "execution futures_house with { orders moc; slippage none }")
    unnamed = PROGRAM.replace("execution futures_house", "execution futures_house with { orders moc }")
    named = unnamed.replace("strategy sma50_es {", "strategy sma50_es_moc {")
    fake_openai.script = [
        ToolCalls([("check", {"program": inline, "name": "a"})]),
        ToolCalls([("check", {"program": foreign, "name": "b"})]),
        ToolCalls([("check", {"program": two_fields, "name": "c"})]),
        ToolCalls([("check", {"program": unnamed, "name": "d"})]),
        ToolCalls([("check", {"program": named, "name": "sma50_es_moc"})]),
        done("could_not_check", "variants"),
    ]
    assert run_harness(built) == 0
    assert fake_gateway.routes_called() == ["check"]
    answers = [r.input_text for r in fake_openai.requests[1:]]
    assert all('"code":"preset"' in a for a in answers[:4])
    assert "named for it" in answers[3]
    assert '"program_hash"' in answers[4]


def test_checks_are_bounded_and_end_as_could_not_check(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    d = doc("post-1", CANDIDATE_TEXT)
    built = build_job(tmp_path, "strategize", [d], hypothesis=hypothesis_record(d["id"]), budget={"checks": 3})
    buggy = PROGRAM.replace("rel atr(", "# BUG\n    rel atr(")
    fake_openai.script = [ToolCalls([("check", {"program": buggy, "name": "sma50_es"})]) for _ in range(4)] + [done("submitted")]
    assert run_harness(built) == 0
    status = built.status()
    assert status["state"] == "failed" and status["reason"] == "could_not_check"
    assert "3 check iterations" in status["detail"]
    assert fake_gateway.routes_called() == ["check", "check", "check"]
    assert "check 1/3" in built.notes()


def test_zero_decisions_is_a_failure(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = quant_job(tmp_path)
    silent = PROGRAM.replace("# The author's", "# NO_TRADES The author's")
    fake_openai.script = [
        ToolCalls([("check", {"program": silent, "name": "sma50_es"})]),
        ToolCalls([("plan_run", {"plan": DEV_PLAN, "program_name": "sma50_es", "program": silent})]),
        done("could_not_check", "no trades"),
    ]
    assert run_harness(built) == 0
    status = built.status()
    assert status["reason"] == "no_decisions"
    assert "decisions = 0" in built.notes()


def test_a_token_without_the_cap_is_refused_by_the_gateway(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    d = doc("post-1", CANDIDATE_TEXT)
    built = build_job(tmp_path, "strategize", [d], hypothesis=hypothesis_record(d["id"]), caps=["abt.check", "notes.write"])
    fake_openai.script = [
        ToolCalls([("check", {"program": PROGRAM, "name": "sma50_es"})]),
        done("not_testable", "the token has no plan capability"),
    ]
    assert run_harness(built) == 0
    assert sorted(fake_openai.requests[0].tool_names) == ["check", "contract", "note", "show"]
    assert built.status()["reason"] == "not_testable"


def every_request_carried_the_token(fake_gateway, built) -> bool:
    token = (built.job_dir / "in" / "token").read_text().strip()
    return all(t == token for _, t, _ in fake_gateway.requests)


def _fill_plan_hash(fake_openai, fake_gateway, plan: str) -> None:
    """The scripted model names the development plan's hash as the gateway will compute it."""
    import hashlib

    h = hashlib.sha256(plan.encode()).hexdigest()
    for step in fake_openai.script:
        if isinstance(step, ToolCalls):
            for _, args in step.calls:
                if args.get("plan_hash") == "FILLED":
                    args["plan_hash"] = h

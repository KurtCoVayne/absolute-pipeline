"""A budget overrun exits 2 with `budget` in status.json."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from agents_harness.budget import BudgetExceeded, Meter, rate_for
from agents_harness.job import Budget

from .conftest import CANDIDATE_TEXT, build_job, doc
from .fake_openai import Message, ToolCalls


def test_tokens_in_overrun_exits_2(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "triage", [doc("post-1", CANDIDATE_TEXT)])
    fake_openai.script = [Message(json.dumps({"verdict": "none", "instruments": [], "topics": [], "score": 0.0}), input_tokens=50_000)]
    assert run_harness(built) == 2
    status = built.status()
    assert status["state"] == "failed" and status["reason"] == "budget" and status["retryable"] is False
    assert "tokens_in" in status["detail"]
    assert built.records() == []
    assert built.cost()["tokens_in"] == 50_000


def test_tool_calls_overrun_exits_2(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "triage", [doc("post-1", CANDIDATE_TEXT)], budget={"tool_calls": 1})
    fake_openai.script = [
        ToolCalls([("note", {"text": "one"}), ("note", {"text": "two"})]),
        Message(json.dumps({"verdict": "none", "instruments": [], "topics": [], "score": 0.0})),
    ]
    assert run_harness(built) == 2
    assert built.status()["reason"] == "budget"
    assert "tool_calls" in built.status()["detail"]


def test_money_overrun_exits_2(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "triage", [doc("post-1", CANDIDATE_TEXT)], budget={"money_usd": 0.00001, "tokens_in": 1_000_000})
    fake_openai.script = [Message(json.dumps({"verdict": "none", "instruments": [], "topics": [], "score": 0.0}), input_tokens=5_000)]
    assert run_harness(built) == 2
    assert "money_usd" in built.status()["detail"]


def test_meter_prices_at_the_rate_card() -> None:
    m = Meter(budget=Budget(1_000_000, 100_000, 10, 60, 1.0), model="gpt-6-luna", rate=rate_for("gpt-6-luna"))
    m.add_usage(100_000, 10_000, cached_tokens=50_000)
    # 50k uncached at $0.10, 50k cached at $0.01, 10k out at $0.50 per million.
    assert m.money_usd == pytest.approx(0.005 + 0.0005 + 0.005)
    assert rate_for("gpt-6.1-sol").input == 2.0
    assert rate_for("unknown-model").output == 10.0
    with pytest.raises(BudgetExceeded) as e:
        m.add_usage(2_000_000, 0)
    assert e.value.which == "tokens_in"

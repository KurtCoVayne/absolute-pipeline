"""One live smoke test per agent on gpt-6-luna through the real API (marked
`live`; runs only when SECRETS/openai_api_key exists). The gateway is the fake
one, so the quant's smoke test checks that the loop runs under the harness's
guards and ends with a status, not that a submission is made. Each test is
capped under $0.10 by the job's budget."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from .conftest import CANDIDATE_TEXT, build_job, doc, secrets_key
from .quant_fixtures import hypothesis_record

pytestmark = pytest.mark.live

KEY = secrets_key()
skip_without_key = pytest.mark.skipif(KEY is None, reason="SECRETS/openai_api_key is absent")


@pytest.fixture
def live(monkeypatch: pytest.MonkeyPatch, fake_gateway):
    assert KEY is not None
    monkeypatch.setenv("OPENAI_BASE_URL", "https://api.openai.com/v1")
    monkeypatch.setenv("OPENAI_API_KEY", KEY)
    monkeypatch.setenv("ABT_GATEWAY", fake_gateway.base_url)

    def run(built) -> int:
        from agents_harness.__main__ import main

        monkeypatch.setenv("JOB_DIR", str(built.job_dir))
        return main([str(built.job_dir)])

    return run


@skip_without_key
def test_live_triage(tmp_path: Path, live) -> None:
    built = build_job(tmp_path, "triage", [doc("post-1", CANDIDATE_TEXT)])
    code = live(built)
    status = built.status()
    print("live triage:", status, built.cost())
    assert code == 0 and status["state"] == "done", status
    [record] = built.records()
    assert record["verdict"] in {"Claim", "Candidate"}
    assert built.cost()["money_usd"] < 0.10


@skip_without_key
def test_live_analyst(tmp_path: Path, live) -> None:
    built = build_job(tmp_path, "analyze", [doc("post-1", CANDIDATE_TEXT)])
    code = live(built)
    status = built.status()
    print("live analyst:", status, built.cost(), len(built.records()), "hypotheses")
    assert code == 0 and status["state"] == "done", status
    records = built.records()
    assert 1 <= len(records) <= 3
    assert all(r["h"] == built.job["h"] for r in records)
    assert built.cost()["money_usd"] < 0.10


@skip_without_key
def test_live_quant(tmp_path: Path, live, fake_gateway) -> None:
    d = doc("post-1", CANDIDATE_TEXT)
    built = build_job(tmp_path, "strategize", [d], hypothesis=hypothesis_record(d["id"]), budget={"money_usd": 0.09, "tool_calls": 40, "wall_s": 600})
    code = live(built)
    status = built.status()
    print("live quant:", status, built.cost(), "routes:", fake_gateway.routes_called())
    print(built.notes()[:3000])
    assert code in (0, 2)
    assert status["state"] in {"done", "failed"}
    if status["state"] == "failed":
        assert status["reason"] in {"not_testable", "no_data", "could_not_check", "no_decisions", "budget"}
    assert built.cost()["money_usd"] < 0.10
    assert "submit" not in fake_gateway.routes_called() or built.records()[0]["record"] == "Submission"
    (tmp_path / "live-quant.json").write_text(json.dumps({"status": status, "cost": built.cost(), "routes": fake_gateway.routes_called()}, indent=1))

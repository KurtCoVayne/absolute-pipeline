"""A triage job writes a valid Triage record; the prompt holds the skills and the envelopes."""

from __future__ import annotations

import json
from pathlib import Path

from .conftest import CANDIDATE_TEXT, SKILLS_ROOT, build_job, doc
from .fake_openai import Message, Status, ToolCalls


def test_triage_writes_a_valid_record(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "triage", [doc("post-1", CANDIDATE_TEXT)])
    fake_openai.script = [Message(json.dumps({"verdict": "candidate", "instruments": ["ES"], "topics": ["trend", "sma-crossover"], "score": 0.8}))]

    assert run_harness(built) == 0
    assert built.status() == {"state": "done", "reason": "", "retryable": False, "detail": ""}
    [record] = built.records()
    assert record["record"] == "Triage"
    assert record["id"] == "" and record["r"] == 0
    assert record["doc_id"] == built.docs[0]["id"]
    assert record["doc_version"] == 1
    assert record["verdict"] == "Candidate"
    assert record["instruments"] == ["ES"]
    assert record["model"] == "gpt-6-luna"
    assert len(record["prompt_hash"]) == 64
    assert record["job_id"] == built.job["id"]
    assert 0 < record["cost_usd"] < 0.002
    cost = built.cost()
    assert cost["tokens_in"] == 1000 and cost["tokens_out"] == 100 and cost["model"] == "gpt-6-luna"
    assert cost["tool_calls"] == 0 and cost["money_usd"] == record["cost_usd"]

    [req] = fake_openai.requests
    assert req.headers.get("x-pipeline-job") == built.job["id"]
    assert req.body["model"] == "gpt-6-luna"
    guideline = (SKILLS_ROOT.parent / "docs" / "design" / "agent-guidelines.md").read_text()
    assert guideline.strip() in req.instructions
    assert "<!-- skill triage v" in req.instructions
    assert f'<document id="{built.docs[0]["id"]}"' in req.input_text and "untrusted>" in req.input_text
    assert sorted(req.tool_names) == ["note", "read_document"]
    assert req.body["text"]["format"]["type"] == "json_schema"
    assert fake_gateway.requests == []
    # A clean document inside the harness's own envelope is not an injection.
    assert built.notes() == ""
    assert "yours to read" in req.instructions


def test_triage_tools_are_only_the_granted_ones(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "triage", [doc("post-1", CANDIDATE_TEXT)], caps=["notes.write"])
    fake_openai.script = [
        ToolCalls([("note", {"text": "reading"})]),
        Message(json.dumps({"verdict": "claim", "instruments": [], "topics": ["result-only"], "score": 0.3})),
    ]
    assert run_harness(built) == 0
    assert fake_openai.requests[0].tool_names == ["note"]
    assert built.records()[0]["verdict"] == "Claim"
    assert "reading" in built.notes()
    assert built.cost()["tool_calls"] == 1


def test_a_429_is_retried_not_failed(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "triage", [doc("post-1", CANDIDATE_TEXT)])
    fake_openai.script = [
        Status(429, headers={"Retry-After": "0"}),
        Status(503, {"error": {"message": "overloaded"}}),
        Message(json.dumps({"verdict": "none", "instruments": [], "topics": [], "score": 0.0})),
    ]
    assert run_harness(built) == 0
    assert built.status()["state"] == "done"
    assert len(fake_openai.requests) == 3
    assert built.records()[0]["verdict"] == "None"

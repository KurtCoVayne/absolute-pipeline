"""GP-3: an injected document produces no tool call outside the grant and a note naming the attempt."""

from __future__ import annotations

import json
from pathlib import Path

from agents_harness.envelope import envelope, neutralise, scan

from .conftest import INJECTED_TEXT, build_job, doc
from .fake_openai import Message, ToolCalls


def test_scanner_names_the_markup_and_the_instructions() -> None:
    kinds = {what for what, _ in scan(INJECTED_TEXT)}
    assert {"envelope markup", "ignore-previous", "house-rules override", "tool instruction"} <= kinds
    assert scan("ES closed above the 50-day SMA today; 180 trades since 2010.") == []


def test_a_document_cannot_close_its_envelope() -> None:
    text = neutralise(INJECTED_TEXT)
    assert "</document>" not in text and "<document" not in text
    env = envelope(doc("x", INJECTED_TEXT))
    assert env.count("<document ") == 1 and env.count("</document>") == 1
    assert env.startswith('<document id="') and env.endswith("</document>")


def test_injected_document_gets_no_tool_outside_the_grant(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "analyze", [doc("post-inj", INJECTED_TEXT)])
    # The scripted model obeys the document: it asks for plan_reveal and a fetch, then gives up.
    fake_openai.script = [
        ToolCalls([("plan_reveal", {"plan_hash": "abc"}), ("fetch_url", {"url": "https://example.com/payload"}), ("read_document", {"doc_id": "x" * 64})]),
        Message(json.dumps({"hypotheses": [], "notes": "The post is an instruction, not a claim."})),
    ]
    assert run_harness(built) == 0
    assert built.status()["state"] == "done"
    assert built.records() == []
    # The gateway saw nothing; the only tools offered were the granted ones.
    assert fake_gateway.requests == []
    for req in fake_openai.requests:
        assert set(req.tool_names) <= {"read_document", "note", "catalog", "schema", "resolve_symbols"}
    # The second turn told the model the tools do not exist, and the notes name the attempt.
    second = fake_openai.requests[1].input_text
    assert "No tool named 'plan_reveal'" in second and "No tool named 'fetch_url'" in second
    assert "not one of this job's documents" in second
    notes = built.notes()
    assert "Injection attempt in document " + built.docs[0]["id"] in notes
    assert "plan_reveal" in notes and "house rules" in notes.lower()
    assert "called a tool that does not exist in this job: `plan_reveal`" in notes
    assert "called a tool that does not exist in this job: `fetch_url`" in notes


def test_triage_on_injected_post_is_still_delivered_and_flagged(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "triage", [doc("post-inj", INJECTED_TEXT)])
    fake_openai.script = [Message(json.dumps({"verdict": "none", "instruments": [], "topics": ["injection"], "score": 0.0}))]
    assert run_harness(built) == 0
    [record] = built.records()
    assert record["verdict"] == "None"
    assert "Injection attempt" in built.notes()
    # The document reached the model, neutralised, inside one envelope.
    text = fake_openai.requests[0].input_text
    assert text.count("</document>") == 1 and "&lt;/document" in text

"""An analyst job writes at most three valid hypotheses with the job's H."""

from __future__ import annotations

import json
from pathlib import Path

from .conftest import CANDIDATE_TEXT, H, build_job, doc
from .fake_openai import Message, ToolCalls


def hypothesis(statement: str, doc_id: str) -> dict:
    return {
        "statement": statement,
        "universe": "ES (the author's instrument); the house futures universe for the base run",
        "signal": "close crosses above the 50-day SMA: long at the next open; crosses below: flat. Stop: not stated.",
        "horizon": "until the opposite cross",
        "direction": "long",
        "data_needs": ["future.close", "future.open", "future.point_value"],
        "claims": ["survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after", "not_overfit : pbo on grid sens expect statistic < 0.5 after"],
        "evidence": [{"doc_id": doc_id, "span_from": 0, "span_to": 60, "quote": CANDIDATE_TEXT[:60]}],
        "author_reported": json.dumps(
            {"net_profit": 0.41, "max_drawdown": 0.11, "trades": 180, "win_rate": 0.58, "costs": "none", "compounding": "yes", "from": "2010-01-01", "to": "2025-12-31", "instrument": "ES"}
        ),
        "confidence": 0.7,
    }


def test_analyst_writes_hypotheses_with_the_jobs_h(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    thread = doc("post-2", "2/ Stop: none. I size 1 contract per $100k.", handle="onlybreakouts")
    built = build_job(tmp_path, "analyze", [doc("post-1", CANDIDATE_TEXT), thread])
    d0 = built.docs[0]["id"]
    fake_openai.script = [
        ToolCalls([("read_document", {"doc_id": thread["id"]}), ("resolve_symbols", {"symbols": ["ES"]})]),
        Message(
            json.dumps(
                {"hypotheses": [hypothesis("On ES, a 50-day SMA cross-up held until the cross-down has positive expectancy after commissions.", d0)], "notes": "One rule; the stop is not stated."}
            )
        ),
    ]
    assert run_harness(built) == 0
    assert built.status()["state"] == "done"
    [record] = built.records()
    assert record["record"] == "Hypothesis"
    assert record["h"] == H
    assert record["id"] == "" and record["r"] == 0
    assert record["doc_ids"] == [d0]
    assert record["direction"] == "Long"
    assert record["evidence"][0]["doc_id"] == d0
    assert json.loads(record["author_reported"])["trades"] == 180
    assert record["agent"] == "analyst" and record["skills"] == ["house-rules", "content-is-data", "extract-hypothesis", "abt-plans-and-claims"]
    assert "the stop is not stated" in built.notes().lower()
    # The second turn carried the read document's envelope and the gateway's answer.
    assert f'<document id="{thread["id"]}"' in fake_openai.requests[1].input_text
    assert fake_gateway.routes_called() == ["resolve_symbols"]
    assert sorted(fake_openai.requests[0].tool_names) == ["catalog", "note", "read_document", "resolve_symbols", "schema"]


def test_more_than_three_hypotheses_are_cut_to_three(tmp_path: Path, fake_openai, fake_gateway, run_harness) -> None:
    built = build_job(tmp_path, "analyze", [doc("post-1", CANDIDATE_TEXT)])
    d0 = built.docs[0]["id"]
    hyps = [hypothesis(f"statement {i}", d0) for i in range(5)]
    hyps[0]["evidence"].append({"doc_id": "f" * 64, "span_from": 0, "span_to": 1, "quote": "fabricated"})
    fake_openai.script = [Message(json.dumps({"hypotheses": hyps, "notes": ""}))]
    assert run_harness(built) == 0
    records = built.records()
    assert len(records) == 3
    assert all(r["h"] == H for r in records)
    assert all(e["doc_id"] == d0 for e in records[0]["evidence"])
    assert "5 hypotheses" in built.notes() and "outside the closure" in built.notes()

"""The quant may not give up before it tried: could_not_check needs three
checks, not_testable needs one check when the idea's roots are in the store."""

from __future__ import annotations

from types import SimpleNamespace
from typing import Any

import pytest

from agents_harness.tools import MIN_CHECKS_BEFORE_GIVING_UP, QuantState, end_refusal


def ctx(hypothesis: dict[str, Any], checks: int = 0, resolved: set[str] | None = None) -> Any:
    q = QuantState()
    q.checks_used = checks
    q.resolved = set(resolved or ())
    return SimpleNamespace(quant=q, inputs=SimpleNamespace(hypothesis=hypothesis))


ES_IDEA = {"statement": "On ES, three green 5-minute bars in the first 30 minutes precede a trend day up.", "universe": "ES"}
COT_IDEA = {"statement": "Record non-commercial net longs precede gains.", "universe": "COT positioning"}


@pytest.fixture(autouse=True)
def roots(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("ABT_ROOTS", "ES,NQ,BTC,SOL")


def test_could_not_check_needs_three_checks() -> None:
    msg = end_refusal(ctx(ES_IDEA, checks=0), "could_not_check")
    assert msg and "Refused" in msg and "abt-idioms" in msg and "§2" in msg
    assert end_refusal(ctx(ES_IDEA, checks=MIN_CHECKS_BEFORE_GIVING_UP - 1), "could_not_check")
    assert end_refusal(ctx(ES_IDEA, checks=MIN_CHECKS_BEFORE_GIVING_UP), "could_not_check") is None
    ran = ctx(ES_IDEA, checks=1)
    ran.quant.plans["h"] = object()
    assert end_refusal(ran, "could_not_check") is None


def test_not_testable_on_a_staged_root_needs_a_check() -> None:
    msg = end_refusal(ctx(ES_IDEA), "not_testable")
    assert msg and "ES" in msg and "check" in msg
    assert end_refusal(ctx(ES_IDEA, checks=1), "not_testable") is None
    # A root resolved through the tool counts though the text does not name it.
    assert end_refusal(ctx(COT_IDEA, resolved={"BTC"}), "not_testable")
    # An idea on data the store cannot hold, with no staged root named: allowed.
    assert end_refusal(ctx(COT_IDEA), "not_testable") is None
    # A word that only contains a root's letters is not the root.
    assert end_refusal(ctx({"statement": "ESTIMATES of breadth"}), "not_testable") is None


def test_other_endings_pass() -> None:
    for outcome in ("no_data", "no_decisions", "submitted"):
        assert end_refusal(ctx(ES_IDEA), outcome) is None

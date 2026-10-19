"""The checked rules read from the texts (guards.py)."""

from __future__ import annotations

import pytest

from agents_harness import guards

from .quant_fixtures import DEV_PLAN, FORWARD_PLAN, PROGRAM, RATIONALE, TRANSLATION_NOTE

H = "2026-09-18T19:12:36Z"


def test_h_text_renders_to_the_second_in_utc() -> None:
    assert guards.h_text("2026-09-18T19:12:36.653000000Z") == H
    assert guards.h_text("2026-09-18T21:12:36+02:00") == H
    assert guards.h_text("2026-09-18T19:12:36Z") == H
    with pytest.raises(guards.Refused):
        guards.h_text("yesterday")


def test_execution_presets_and_variants() -> None:
    assert guards.check_execution(PROGRAM).preset == "futures_house"
    eq = PROGRAM.replace("execution futures_house", "execution equities_house")
    assert guards.check_execution(eq).preset == "equities_house"
    variant = PROGRAM.replace("strategy sma50_es {", "strategy sma50_es_no_costs {").replace("execution futures_house", "execution futures_house with { commission none }")
    assert guards.check_execution(variant).overrides == {"commission": "none"}
    for bad in (
        PROGRAM.replace("execution futures_house", "execution futures_fixed_base"),
        PROGRAM.replace("execution futures_house", "execution futures_house with { commission none }"),
        PROGRAM.replace("execution futures_house", "execution futures_house with { nonsense 1 }"),
        PROGRAM.replace("execution futures_house", "execution {\n price close\n}"),
        PROGRAM.replace("execution futures_house", ""),
    ):
        with pytest.raises(guards.Refused) as e:
            guards.check_execution(bad)
        assert e.value.code == "preset"


def test_hold_outs_and_claims() -> None:
    assert guards.plan_holdout(DEV_PLAN, H).kind == "development"
    assert guards.plan_holdout(FORWARD_PLAN, H).kind == "forward"
    with pytest.raises(guards.Refused):
        guards.plan_holdout(FORWARD_PLAN, "2026-09-19T00:00:00Z")
    with pytest.raises(guards.Refused):
        guards.plan_holdout(DEV_PLAN.replace("trailing:20%", "blocks:4:6mo"), H)
    assert guards.claims_phases(DEV_PLAN) == ["after", "after"]
    guards.require_after_claims(FORWARD_PLAN)
    with pytest.raises(guards.Refused):
        guards.require_after_claims(DEV_PLAN.replace(" after\n", " before\n"))
    assert guards.plan_program(DEV_PLAN) == "sma50_es.dsl"
    assert guards.run_steps(DEV_PLAN) == ["sanity", "base", "long_only"]
    guards.forbid_reveal(FORWARD_PLAN)
    with pytest.raises(guards.Refused):
        guards.forbid_reveal(DEV_PLAN)


def test_revises_header() -> None:
    assert guards.revises(PROGRAM) is None
    assert guards.revises("# revises: " + "a" * 64 + "\n" + PROGRAM) == "a" * 64


def test_translation_check_and_comparison_table() -> None:
    assert guards.has_translation_check([TRANSLATION_NOTE])
    assert not guards.has_translation_check(["## Translation check\nlooks fine"])
    assert not guards.has_translation_check(["P&L = 11015.72 USD, commission 27.04"])
    assert guards.has_comparison_table(RATIONALE)
    assert not guards.has_comparison_table("| Window | Return |\n| --- | --- |\n| Full | 7 |\n")
    assert not guards.has_comparison_table("| Measure | Author | Ours |\n| --- | --- | --- |\n| Trades | 1 | 2 |\n")

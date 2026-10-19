"""The token economy: cached input weighs a tenth in the cap; tool answers are compact."""

from __future__ import annotations

import json

import pytest

from agents_harness.budget import BudgetExceeded, Meter, rate_for
from agents_harness.job import Budget
from agents_harness.local import brief_outcome
from agents_harness.tools import LIST_ITEMS, TABLE_ROWS, TEXT_CHARS, _dumps, compact


def meter(tokens_in: int, money: float = 10.0) -> Meter:
    b = Budget(tokens_in=tokens_in, tokens_out=1_000_000, tool_calls=1000, wall_s=3600, money_usd=money)
    return Meter(budget=b, model="gpt-6-luna", rate=rate_for("gpt-6-luna"))


def test_cached_input_counts_a_tenth_against_the_cap() -> None:
    m = meter(1_000_000)
    # 3M input of which 2.9M cached: 100k uncached + 290k weighted = 390k, under the cap.
    m.add_usage(3_000_000, 1_000, cached_tokens=2_900_000)
    assert m.tokens_in_weighted == pytest.approx(390_000)
    assert m.cost()["tokens_in_weighted"] == 390_000
    # Uncached input still counts in full.
    with pytest.raises(BudgetExceeded) as e:
        m.add_usage(700_000, 0, cached_tokens=0)
    assert e.value.which == "tokens_in"


def test_the_money_cap_still_binds() -> None:
    m = meter(10_000_000, money=0.01)
    with pytest.raises(BudgetExceeded) as e:
        m.add_usage(2_000_000, 0, cached_tokens=1_000_000)
    assert e.value.which == "money_usd"


def test_tables_lists_texts_and_diagnostics_are_compacted() -> None:
    table = {"columns": ["A", "T", "P"], "rows": [[i, i, i] for i in range(500)], "query": "q"}
    c = compact({"result": table})
    assert len(c["result"]["rows"]) == TABLE_ROWS and c["result"]["rows_total"] == 500 and c["result"]["truncated"]
    assert compact("x" * (TEXT_CHARS + 10)).endswith("[10 more characters]")
    assert len(compact(list(range(LIST_ITEMS + 5)))) == LIST_ITEMS + 1
    diags = [{"code": "W9", "message": "hindsight list"}] * 7 + [{"code": "T", "message": "a type error"}]
    d = compact(diags)
    assert d == [{"code": "W9", "message": "hindsight list", "count": 7}, {"code": "T", "message": "a type error"}]
    assert "normal_form" not in compact({"hash": "h", "normal_form": "long"})
    assert len(_dumps({"t": table})) < 4_000


def test_an_outcome_is_briefed_to_its_metric_rows() -> None:
    o = {
        "step": "base",
        "point": [["n", "20"]],
        "run_id": "r",
        "binding": "b" * 64,
        "warnings": ["w"] * 30,
        "returns": {"sharpe": 0.123456789, "cagr": 0.05, "skew": 1.0, "kurtosis": 3.0, "max_drawdown": 0.2},
        "trading": {"decisions": 12, "fills": 11, "avg_gross": 1e6},
        "trades": {"count": 6, "profit_factor": 1.4},
    }
    b = brief_outcome(o)
    assert b["returns"] == {"cagr": 0.05, "sharpe": 0.12346, "max_drawdown": 0.2}
    assert b["trading"] == {"decisions": 12, "fills": 11}
    assert b["trades"] == {"count": 6, "profit_factor": 1.4}
    assert "binding" not in b and "warnings" not in b
    assert len(json.dumps(b)) < len(json.dumps(o)) / 2

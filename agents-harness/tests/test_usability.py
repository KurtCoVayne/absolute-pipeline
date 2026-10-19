"""The usability runner (GP-5) over the fakes: a dry run prints the prompt and
calls nothing; a scripted run saves every attempt and its diagnostics, and the
scorer reads them."""

from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

from .conftest import REPO
from .fake_gateway import fake_token
from .fake_openai import Message, ToolCalls

USABILITY = REPO / "agents-harness" / "usability"
sys.path.insert(0, str(USABILITY))

import run as usability_run  # noqa: E402
import score as usability_score  # noqa: E402

R8L_LIKE = """strategy r8l {
  data { class future   need bars @1m from 2000-01-01 complete   need sessions, point_value, commission   require reviewed   prefer latest }
  uses calendar, bars
  calendars { session = sessions   es_days = sessions of "ES" }
  clock { decide on base at session + 30min }
  universe { universe(A, T) :- member(A, T, "HOUSE"), session_of(A, T, _). }
  params { risk : Notional<USD> = 6493.912071090746 USD }
  rules {
    rel atr(A: Future, @S: session, V: Price<USD>)
    atr(A, S, V) :- session(A, S, _), V = mean(R) over (S1 in rows[20], true_range@session(A, S1, R)).
  }
  decisions { silence flat   orders moo_moc   decide(T, hold(A, N)) :- atr(A, T, V), point_value(A, T, PV), N = risk / (V * PV). }
  execution futures_house
  report { calendar es_days   periods 252 per year }
}
"""


def test_dry_run_prints_the_prompt_and_calls_nothing(tmp_path: Path, capsys: pytest.CaptureFixture[str], fake_gateway, fake_openai) -> None:
    assert usability_run.main(["--model", "gpt-6-luna", "--dry-run", "--out", str(tmp_path)]) == 0
    out = capsys.readouterr().out
    assert "=== system instructions ===" in out and "<!-- skill abt-language v" in out and "<!-- skill abt-idioms v" in out
    assert "# TASK.md" in out and "R8L intraday futures opening-range book" in out
    assert "house-rules" not in out.split("=== user prompt ===")[0].replace("house rules", "")
    assert fake_gateway.requests == [] and fake_openai.requests == []
    [run_dir] = [p for p in tmp_path.iterdir() if p.is_dir()]
    assert (run_dir / "job" / "in" / "definition.toml").is_file()


def test_scripted_run_saves_attempts_and_scores(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, fake_gateway, fake_openai) -> None:
    token_file = tmp_path / "token"
    token_file.write_text(fake_token(["abt.check", "notes.write"], "2026-08-14T21:00:00Z") + "\n")
    monkeypatch.setenv("OPENAI_BASE_URL", fake_openai.base_url)
    monkeypatch.setenv("OPENAI_API_KEY", "sandbox")
    buggy = R8L_LIKE.replace("rel atr(", "# BUG\n    rel atr(")
    fake_openai.script = [
        ToolCalls([("check", {"program": buggy, "name": "r8l"})]),
        ToolCalls([("check", {"program": R8L_LIKE, "name": "r8l"})]),
        Message(json.dumps({"program": R8L_LIKE, "checks_clean": True, "notes": "Two attempts; the M diagnostic named the unbound value."})),
    ]
    code = usability_run.main(["--model", "gpt-6-luna", "--out", str(tmp_path / "runs"), "--gateway", fake_gateway.base_url, "--token-file", str(token_file)])
    assert code == 0
    [run_dir] = list((tmp_path / "runs").iterdir())
    assert sorted(p.name for p in (run_dir / "attempts").iterdir()) == ["1.dsl", "2.dsl"]
    assert (run_dir / "attempts" / "2.dsl").read_text() == R8L_LIKE
    assert json.loads((run_dir / "check" / "1.json").read_text())["diagnostics"][0]["code"] == "M"
    assert (run_dir / "final.dsl").read_text() == R8L_LIKE
    assert "NOTES.md" in (run_dir / "notes.md").read_text()
    result = json.loads((run_dir / "result.json").read_text())
    assert result["status"] == "done" and result["checks"] == 2 and result["cost"]["tool_calls"] == 2
    assert fake_gateway.routes_called() == ["check", "check"]
    assert sorted(fake_openai.requests[0].tool_names) == ["check", "contract", "note", "show"]

    s = usability_score.score(run_dir, run_decisions=21892)
    assert s["attempts"] == 2 and s["first_clean_check"] == 2 and s["final_is_a_clean_attempt"] is True and s["trades"] is True
    (run_dir / "run.json").write_text(json.dumps({"decisions": 0}))
    assert usability_score.score(run_dir)["trades"] is False
    assert usability_score.main([str(run_dir)]) == 0
    assert (run_dir / "score.json").is_file()

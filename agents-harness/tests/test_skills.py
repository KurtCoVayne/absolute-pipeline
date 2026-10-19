"""The skills and the definitions (contracts section 7, PL-6.2 to PL-6.4)."""

from __future__ import annotations

import json
import re

import blake3
import pytest

from agents_harness.skills import CLOSED_CAPS, SkillError, blake3_hex, load_definition, parse_skill, skills_hash

from .conftest import REPO, SKILLS_ROOT

NINE = ["house-rules", "content-is-data", "triage", "extract-hypothesis", "abt-language", "abt-idioms", "abt-plans-and-claims", "write-strategy", "read-results"]


def test_house_rules_is_the_guideline_byte_for_byte() -> None:
    skill = (SKILLS_ROOT / "skills" / "house-rules" / "SKILL.md").read_bytes()
    guideline = (REPO / "docs" / "design" / "agent-guidelines.md").read_bytes()
    front = b"---\nname: house-rules\nversion: 1\n---\n"
    assert skill.startswith(front)
    assert skill[len(front) :] == guideline


def test_the_nine_skills_exist_with_front_matter() -> None:
    for name in NINE:
        s = parse_skill((SKILLS_ROOT / "skills" / name / "SKILL.md").read_bytes(), name)
        assert s.name == name and s.version
        assert len(s.body) > 500
    assert sorted(p.name for p in (SKILLS_ROOT / "skills").iterdir() if p.is_dir()) == sorted(NINE)


def test_every_definition_is_inside_the_closed_set() -> None:
    for agent in ("triage", "analyst", "quant"):
        d = load_definition(SKILLS_ROOT / "definitions" / f"{agent}.toml")
        assert set(d.caps) <= CLOSED_CAPS
        assert d.model.id == "gpt-6-luna" and d.model.provider == "openai"
        assert re.fullmatch(r"\d{4}-\d{2}-\d{2}", d.model.knowledge_cutoff)
        assert all(s in NINE for s in d.skills)
        assert d.skills[0] == "house-rules" and "content-is-data" in d.skills
        assert len(d.hash) == 64
        for s in d.skills:
            assert (SKILLS_ROOT / "skills" / s / "SKILL.md").is_file()


def test_caps_and_budgets_are_the_plans() -> None:
    triage = load_definition(SKILLS_ROOT / "definitions" / "triage.toml")
    analyst = load_definition(SKILLS_ROOT / "definitions" / "analyst.toml")
    quant = load_definition(SKILLS_ROOT / "definitions" / "quant.toml")
    assert triage.caps == ("docs.read", "notes.write") and triage.budget["money_usd"] <= 0.002
    assert analyst.caps == ("docs.read", "abt.catalog", "notes.write") and analyst.budget["money_usd"] <= 0.05
    assert quant.caps == ("docs.read", "abt.catalog", "abt.query", "abt.check", "abt.plan", "abt.report", "abt.query_run", "abt.submit", "notes.write")
    assert quant.budget["money_usd"] <= 0.25 and quant.budget["plan_runs"] == 4 and quant.budget["grid_points"] == 25 and quant.budget["checks"] == 8
    assert triage.records == ("Triage",) and analyst.records == ("Hypothesis",) and quant.records == ("Submission",)
    assert "provenance.write" not in CLOSED_CAPS


def test_skills_hash_is_blake3_of_the_sorted_canonical_list() -> None:
    skills = [("b-skill", b"two"), ("a-skill", b"one")]
    expected_list = [["a-skill", blake3.blake3(b"one").hexdigest()], ["b-skill", blake3.blake3(b"two").hexdigest()]]
    expected = blake3.blake3(json.dumps(expected_list, separators=(",", ":"), sort_keys=True).encode()).hexdigest()
    assert skills_hash(skills) == expected
    assert skills_hash(list(reversed(skills))) == expected
    assert skills_hash([("a-skill", b"one")]) != expected
    assert blake3_hex(b"") == "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"


def test_the_repository_skills_hash_is_stable_across_orderings() -> None:
    items = [(n, (SKILLS_ROOT / "skills" / n / "SKILL.md").read_bytes()) for n in NINE]
    assert skills_hash(items) == skills_hash(sorted(items))


def test_the_job_directorys_skill_wins_over_the_fallback(tmp_path) -> None:
    from agents_harness.skills import load_skills

    fallback = tmp_path / "opt-skills"
    (fallback / "abt-idioms").mkdir(parents=True)
    (fallback / "abt-idioms" / "SKILL.md").write_bytes(b"---\nname: abt-idioms\nversion: 9\n---\nfallback body\n")
    (fallback / "triage").mkdir()
    (fallback / "triage" / "SKILL.md").write_bytes(b"---\nname: triage\nversion: 9\n---\nfallback triage\n")
    job = tmp_path / "job"
    (job / "in" / "skills" / "triage").mkdir(parents=True)
    (job / "in" / "skills" / "triage" / "SKILL.md").write_bytes(b"---\nname: triage\nversion: 1\n---\njob body\n")
    skills = load_skills(("triage", "abt-idioms"), job, fallback)
    assert [(s.name, s.version, s.body.strip()) for s in skills] == [("triage", "1", "job body"), ("abt-idioms", "9", "fallback body")]
    with pytest.raises(SkillError):
        load_skills(("house-rules",), job, fallback)

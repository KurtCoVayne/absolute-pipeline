"""The job directory (contracts section 3): what the orchestrator mounted at `/job/in`."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .skills import Definition, Skill, load_definition, load_skills

JOB_DIR_DEFAULT = Path("/job")


class JobError(ValueError):
    pass


@dataclass(frozen=True)
class Budget:
    tokens_in: int
    tokens_out: int
    tool_calls: int
    wall_s: float
    money_usd: float
    plan_runs: int = 0
    grid_points: int = 0
    checks: int = 0

    @classmethod
    def of(cls, raw: dict[str, Any]) -> Budget:
        try:
            return cls(
                tokens_in=int(raw["tokens_in"]),
                tokens_out=int(raw["tokens_out"]),
                tool_calls=int(raw["tool_calls"]),
                wall_s=float(raw["wall_s"]),
                money_usd=float(raw["money_usd"]),
                plan_runs=int(raw.get("plan_runs", 0)),
                grid_points=int(raw.get("grid_points", 0)),
                checks=int(raw.get("checks", 0)),
            )
        except (KeyError, TypeError, ValueError) as e:
            raise JobError(f"the budget is incomplete: {e}") from e


@dataclass(frozen=True)
class Job:
    id: str
    stage: str
    agent: str
    model: str
    caps: tuple[str, ...]
    budget: Budget
    h: str
    knowledge_cutoff: str
    lineage: str
    campaign: str
    inputs: tuple[str, ...]
    raw: dict[str, Any] = field(repr=False)

    @classmethod
    def of(cls, raw: dict[str, Any]) -> Job:
        try:
            return cls(
                id=str(raw["id"]),
                stage=str(raw["stage"]).lower(),
                agent=str(raw["agent"]),
                model=str(raw["model"]),
                caps=tuple(str(c) for c in raw.get("caps", [])),
                budget=Budget.of(raw["budget"]),
                h=str(raw["h"]),
                knowledge_cutoff=str(raw.get("knowledge_cutoff", "")),
                lineage=str(raw.get("lineage", "") or ""),
                campaign=str(raw.get("campaign", "") or ""),
                inputs=tuple(str(i) for i in raw.get("inputs", [])),
                raw=raw,
            )
        except KeyError as e:
            raise JobError(f"job.json lacks {e}") from e


@dataclass
class Inputs:
    job_dir: Path
    job: Job
    token: str
    definition: Definition
    skills: list[Skill]
    docs: dict[str, dict[str, Any]]
    closure: list[str]
    hypothesis: dict[str, Any] | None

    @property
    def out_dir(self) -> Path:
        return self.job_dir / "out"

    def doc_text(self, doc_id: str) -> str:
        return str(self.docs[doc_id].get("text", ""))


def job_dir_from_env() -> Path:
    return Path(os.environ.get("JOB_DIR", str(JOB_DIR_DEFAULT)))


def load_inputs(job_dir: Path, skills_fallback: Path | None = None) -> Inputs:
    inp = job_dir / "in"
    if not inp.is_dir():
        raise JobError(f"{inp} is not a directory")
    try:
        job = Job.of(json.loads((inp / "job.json").read_text("utf-8")))
    except (OSError, json.JSONDecodeError) as e:
        raise JobError(f"job.json: {e}") from e
    try:
        token = (inp / "token").read_text("utf-8").strip()
    except OSError as e:
        raise JobError(f"token: {e}") from e
    definition = load_definition(inp / "definition.toml")
    if definition.stage != job.stage:
        raise JobError(f"the definition is for stage {definition.stage!r}, the job for {job.stage!r}")
    kwargs = {} if skills_fallback is None else {"fallback": skills_fallback}
    skills = load_skills(definition.skills, job_dir, **kwargs)
    docs_dir = inp / "docs"
    docs: dict[str, dict[str, Any]] = {}
    closure: list[str] = []
    if docs_dir.is_dir():
        closure_file = docs_dir / "closure.json"
        if closure_file.is_file():
            c = json.loads(closure_file.read_text("utf-8"))
            closure = [str(i) for i in c.get("closure", [])]
        for p in sorted(docs_dir.glob("*.json")):
            if p.name == "closure.json":
                continue
            d = json.loads(p.read_text("utf-8"))
            docs[str(d.get("id", p.stem))] = d
        if not closure:
            closure = sorted(docs)
    hypothesis = None
    hp = inp / "hypothesis.json"
    if hp.is_file():
        hypothesis = json.loads(hp.read_text("utf-8"))
    return Inputs(
        job_dir=job_dir,
        job=job,
        token=token,
        definition=definition,
        skills=skills,
        docs=docs,
        closure=closure,
        hypothesis=hypothesis,
    )

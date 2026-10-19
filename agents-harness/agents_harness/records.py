"""The structured outputs of the three agents (Pydantic, for the SDK's
structured outputs) and the records they become in `json::render`'s shape
(PL-D2): the fields of `pipeline.fbs`, enums as their variant names, times as
ISO-8601, `id` empty and `r` unset for the orchestrator to compute."""

from __future__ import annotations

import json
from typing import Any, Literal

from pydantic import BaseModel, Field

Verdict = Literal["none", "commentary", "claim", "candidate"]
DirectionName = Literal["long", "short", "both", "unknown"]
QuantOutcome = Literal["submitted", "not_testable", "no_data", "could_not_check", "no_decisions"]

VERDICT_VARIANT = {"none": "None", "commentary": "Commentary", "claim": "Claim", "candidate": "Candidate"}
DIRECTION_VARIANT = {"long": "Long", "short": "Short", "both": "Both", "unknown": "Unknown"}

MAX_HYPOTHESES = 3


class TriageOut(BaseModel):
    """One Triage verdict (the `triage` skill's scale)."""

    verdict: Verdict
    instruments: list[str] = Field(description="the symbols the document is about, as written")
    topics: list[str] = Field(description="two to five short tags")
    score: float = Field(description="confidence in [0, 1] that a quant could test this from the document alone")


class EvidenceOut(BaseModel):
    doc_id: str = Field(description="the id of the document the span is in")
    span_from: int = Field(description="character offset of the quote's start in that document's text")
    span_to: int = Field(description="character offset of the quote's end")
    quote: str = Field(description="the span, verbatim")


class HypothesisOut(BaseModel):
    statement: str = Field(description="one falsifiable sentence with the direction and the horizon in it")
    universe: str
    signal: str = Field(description="the rule in prose and formulas, every number with units, what is not stated marked")
    horizon: str = Field(description="the holding horizon in the author's words")
    direction: DirectionName
    data_needs: list[str] = Field(description="relations by class, e.g. future.close; name what the store lacks")
    claims: list[str] = Field(description="the tests to pre-register in Q-5.1's syntax")
    evidence: list[EvidenceOut]
    author_reported: str = Field(description="the author's numbers as a JSON object text; {} when none")
    confidence: float


class AnalystOut(BaseModel):
    hypotheses: list[HypothesisOut] = Field(description="zero to three")
    notes: str = Field(description="what was extracted, what was left out and why")


class QuantOut(BaseModel):
    outcome: QuantOutcome
    detail: str = Field(description="the reason when not submitted; one line on the submission otherwise")


def _author_reported_text(text: str) -> str:
    try:
        json.loads(text)
        return text
    except ValueError:
        return json.dumps({"raw": text})


def render_triage(out: TriageOut, *, doc_id: str, doc_version: int, model: str, prompt_hash: str, cost_usd: float, job_id: str) -> dict[str, Any]:
    return {
        "record": "Triage",
        "id": "",
        "doc_id": doc_id,
        "doc_version": doc_version,
        "verdict": VERDICT_VARIANT[out.verdict],
        "instruments": list(out.instruments),
        "topics": list(out.topics),
        "score": max(0.0, min(1.0, out.score)),
        "model": model,
        "prompt_hash": prompt_hash,
        "cost_usd": cost_usd,
        "r": 0,
        "job_id": job_id,
    }


def render_hypothesis(
    out: HypothesisOut,
    *,
    doc_ids: list[str],
    h: str,
    agent: str,
    model: str,
    prompt_hash: str,
    skills: list[str],
    cost_usd: float,
    job_id: str,
) -> dict[str, Any]:
    return {
        "record": "Hypothesis",
        "id": "",
        "doc_ids": list(doc_ids),
        "h": h,
        "statement": out.statement,
        "universe": out.universe,
        "signal": out.signal,
        "horizon": out.horizon,
        "direction": DIRECTION_VARIANT[out.direction],
        "data_needs": list(out.data_needs),
        "claims": list(out.claims),
        "evidence": [{"doc_id": e.doc_id, "span_from": max(0, e.span_from), "span_to": max(0, e.span_to), "quote": e.quote} for e in out.evidence],
        "author_reported": _author_reported_text(out.author_reported),
        "confidence": max(0.0, min(1.0, out.confidence)),
        "agent": agent,
        "model": model,
        "prompt_hash": prompt_hash,
        "skills": list(skills),
        "cost_usd": cost_usd,
        "r": 0,
        "job_id": job_id,
    }


def render_submission(
    *,
    hypothesis_id: str,
    program_hash: str,
    plan_hash: str,
    lineage: str,
    h: str,
    claims: list[str],
    insample: dict[str, Any],
    trials_used: int,
    rationale: str,
    program_text: str,
    plan_text: str,
    forward_plan_text: str,
    forward_plan_hash: str,
    agent: str,
    model: str,
    cost_usd: float,
    job_id: str,
    campaign: str,
    skills_hash: str,
) -> dict[str, Any]:
    return {
        "record": "Submission",
        "id": "",
        "hypothesis_id": hypothesis_id,
        "program_hash": program_hash,
        "plan_hash": plan_hash,
        "lineage": lineage,
        "h": h,
        "claims": list(claims),
        "insample": json.dumps(insample, separators=(",", ":"), sort_keys=True),
        "trials_used": trials_used,
        "rationale": rationale,
        "program_text": program_text,
        "plan_text": plan_text,
        "forward_plan_text": forward_plan_text,
        "forward_plan_hash": forward_plan_hash,
        "agent": agent,
        "model": model,
        "cost_usd": cost_usd,
        "r": 0,
        "job_id": job_id,
        "campaign": campaign,
        "skills_hash": skills_hash,
    }

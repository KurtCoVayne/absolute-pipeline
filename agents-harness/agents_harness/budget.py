"""The job's budget (PL-6.1, PL-6.8): tokens in and out, tool calls, wall
time and money at the rate card, enforced by the harness and audited by the
orchestrator. Exceeding one fails the job as `budget` (exit 2)."""

from __future__ import annotations

import os
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .job import Budget


@dataclass(frozen=True)
class Rate:
    """USD per million tokens."""

    input: float
    cached: float
    output: float


# PL-8.5's rate card as reported on Oct 7, 2026: cached input at a tenth.
RATE_CARD: dict[str, Rate] = {
    "gpt-6-luna": Rate(input=0.10, cached=0.01, output=0.50),
    "gpt-6.1-sol": Rate(input=2.00, cached=0.20, output=10.00),
}


def load_rates(path: Path | None = None) -> dict[str, Rate]:
    """The rate card, overridden by `rates.toml` (`PIPELINE_RATES`) when present:
    `[models."gpt-6-luna"] input = 0.10 cached = 0.01 output = 0.50`."""
    rates = dict(RATE_CARD)
    p = path or (Path(os.environ["PIPELINE_RATES"]) if os.environ.get("PIPELINE_RATES") else None)
    if p is not None and p.is_file():
        data = tomllib.loads(p.read_text("utf-8"))
        for model, r in data.get("models", {}).items():
            rates[model] = Rate(float(r["input"]), float(r.get("cached", r["input"] / 10)), float(r["output"]))
    return rates


def rate_for(model: str, rates: dict[str, Rate] | None = None) -> Rate:
    rates = rates or RATE_CARD
    if model in rates:
        return rates[model]
    # An unknown model is priced at the dearest known rate so that the cap holds.
    return max(rates.values(), key=lambda r: r.input + r.output)


class BudgetExceeded(Exception):
    def __init__(self, which: str, used: float, cap: float) -> None:
        super().__init__(f"{which}: {used:g} over the cap {cap:g}")
        self.which = which
        self.used = used
        self.cap = cap


@dataclass
class Meter:
    budget: Budget
    model: str
    rate: Rate
    started: float = field(default_factory=time.monotonic)
    tokens_in: int = 0
    tokens_out: int = 0
    tokens_cached: int = 0
    tool_calls: int = 0
    requests: int = 0

    @property
    def wall_s(self) -> float:
        return time.monotonic() - self.started

    @property
    def remaining_s(self) -> float:
        return max(0.0, self.budget.wall_s - self.wall_s)

    @property
    def tokens_in_weighted(self) -> float:
        """The input the cap on `tokens_in` counts: uncached tokens at full
        weight, cached ones at a tenth (their price). The skills are resent on
        every turn and mostly cached; counting them in full failed jobs as
        `budget` that had spent a few cents (PL-D14). The money cap binds."""
        uncached = max(0, self.tokens_in - self.tokens_cached)
        return uncached + self.tokens_cached / 10

    @property
    def money_usd(self) -> float:
        uncached = max(0, self.tokens_in - self.tokens_cached)
        return (uncached * self.rate.input + self.tokens_cached * self.rate.cached + self.tokens_out * self.rate.output) / 1_000_000

    def add_usage(self, input_tokens: int, output_tokens: int, cached_tokens: int = 0) -> None:
        self.requests += 1
        self.tokens_in += int(input_tokens)
        self.tokens_out += int(output_tokens)
        self.tokens_cached += int(cached_tokens)
        self.check()

    def add_tool_call(self) -> None:
        self.tool_calls += 1
        self.check()

    def check(self) -> None:
        b = self.budget
        if self.tokens_in_weighted > b.tokens_in:
            raise BudgetExceeded("tokens_in", self.tokens_in_weighted, b.tokens_in)
        if self.tokens_out > b.tokens_out:
            raise BudgetExceeded("tokens_out", self.tokens_out, b.tokens_out)
        if self.tool_calls > b.tool_calls:
            raise BudgetExceeded("tool_calls", self.tool_calls, b.tool_calls)
        if self.wall_s > b.wall_s:
            raise BudgetExceeded("wall_s", self.wall_s, b.wall_s)
        if self.money_usd > b.money_usd:
            raise BudgetExceeded("money_usd", self.money_usd, b.money_usd)

    def cost(self) -> dict[str, Any]:
        """`/job/out/cost.json` (contracts section 3)."""
        return {
            "tokens_in": self.tokens_in,
            "tokens_out": self.tokens_out,
            "tokens_cached": self.tokens_cached,
            "tokens_in_weighted": round(self.tokens_in_weighted),
            "tool_calls": self.tool_calls,
            "wall_s": round(self.wall_s, 3),
            "money_usd": round(self.money_usd, 6),
            "model": self.model,
        }

"""The checked rules of the house (guideline sections 4, 9, 11, 14, 15): what
the harness refuses before the gateway sees it, from the texts alone."""

from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import UTC, datetime

HOUSE_PRESETS = frozenset({"futures_house", "equities_house"})

# A variant changes one field of a preset in a program named for that field
# (guideline section 15: `no_costs`, `moo`, `moc`). The names a program may
# carry for each field.
VARIANT_NAMES: dict[str, tuple[str, ...]] = {
    "commission": ("no_costs", "commission"),
    "slippage": ("no_costs", "slippage"),
    "orders": ("orders", "moo", "moc", "moo_moc", "market"),
    "price": ("price",),
    "lot": ("lot",),
    "numerics": ("numerics",),
    "actions": ("actions",),
    "delisting": ("delisting",),
    "leverage": ("leverage",),
    "financing": ("financing",),
    "impact": ("impact",),
    "participation": ("participation",),
    "oversize": ("oversize",),
    "ruin": ("ruin",),
    "settlement": ("settlement",),
    "limits": ("limits",),
}

_STRATEGY = re.compile(r"^\s*strategy\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{", re.M)
_EXEC_PRESET = re.compile(r"^\s*execution\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:with\s*\{(.*?)\})?\s*$", re.M | re.S)
_EXEC_INLINE = re.compile(r"^\s*execution\s*\{", re.M)
_REVISES = re.compile(r"^\s*#\s*revises:\s*([0-9a-f]{64})\b|^\s*revises\s+([0-9a-f]{64})\b", re.M | re.I)
_PROGRAM = re.compile(r"^\s*program\s+(\S+)", re.M)
_HOLDOUT = re.compile(r"^\s*holdout\s+(\S+)(?:\s+(\S+))?", re.M)
_WINDOW = re.compile(r"^\s*window\s+(\S+)\s*\.\.\s*(\S+)", re.M)
_CLAIMS_BLOCK = re.compile(r"^\s*claims\s*\{(.*?)^\s*\}", re.M | re.S)
_CLAIM_LINE = re.compile(r"^\s*[A-Za-z_][A-Za-z0-9_]*\s*:\s*[a-z_]+\s+on\s+.*?\bexpect\b.*?\b(before|after)\b", re.M)
_REVEAL = re.compile(r"\breveal\s+[A-Za-z_]\w*\s+of\b")
_STEP_RUN = re.compile(r"\brun\s+([A-Za-z_]\w*)")
_TABLE_ROW = re.compile(r"^\s*\|.*\|\s*$")
_CENTS = re.compile(r"-?\d[\d,]*\.\d{2}\b")


class Refused(ValueError):
    """A text the harness refuses, with the gateway's code vocabulary where one applies."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


def program_name(program: str) -> str | None:
    m = _STRATEGY.search(program)
    return m.group(1) if m else None


@dataclass(frozen=True)
class ExecutionBlock:
    preset: str
    overrides: dict[str, str]


def execution_block(program: str) -> ExecutionBlock:
    """The program's `execution` line parsed, or `Refused('preset', …)`."""
    if _EXEC_INLINE.search(program):
        raise Refused("preset", "the execution block is written out; name a house preset instead (futures_house, equities_house)")
    m = _EXEC_PRESET.search(program)
    if not m:
        raise Refused("preset", "no `execution <preset>` line: write `execution futures_house` or `execution equities_house`")
    preset = m.group(1)
    overrides: dict[str, str] = {}
    body = m.group(2)
    if body is not None:
        for entry in re.split(r"[;\n]", body):
            entry = entry.split("#", 1)[0].strip()
            if not entry:
                continue
            key, _, value = entry.partition(" ")
            overrides[key.strip()] = value.strip()
    return ExecutionBlock(preset, overrides)


def check_execution(program: str) -> ExecutionBlock:
    """Guideline section 15: a house preset, or one of them with a single field
    changed in a variant named for that field."""
    block = execution_block(program)
    if block.preset not in HOUSE_PRESETS:
        raise Refused("preset", f"`execution {block.preset}` is not a house preset; the presets are futures_house and equities_house")
    if not block.overrides:
        return block
    if len(block.overrides) > 1:
        raise Refused("preset", f"a variant changes one field of the preset, not {len(block.overrides)}: {sorted(block.overrides)}")
    (field, value), *_ = block.overrides.items()
    names = VARIANT_NAMES.get(field)
    if names is None:
        raise Refused("preset", f"`{field}` is not a field of the execution block")
    name = program_name(program) or ""
    if not any(n in name for n in names):
        raise Refused(
            "preset",
            f"a variant of `{field}` is named for it: the program is `{name}`, which carries none of {list(names)}; a cost variant is `<slug>_no_costs`, a timing variant `<slug>_moo` or `<slug>_moc`",
        )
    return ExecutionBlock(block.preset, {field: value})


def revises(program: str) -> str | None:
    m = _REVISES.search(program)
    return (m.group(1) or m.group(2)).lower() if m else None


def plan_program(plan: str) -> str | None:
    m = _PROGRAM.search(plan)
    return m.group(1) if m else None


def h_text(h_iso: str) -> str:
    """`H` as the gateway wants it written in a plan: `YYYY-MM-DDTHH:MM:SSZ` (contracts section 5)."""
    s = h_iso.strip()
    if s.endswith("Z"):
        s = s[:-1]
    if "." in s:
        s = s.split(".", 1)[0]
    try:
        dt = datetime.fromisoformat(s)
    except ValueError as e:
        raise Refused("holdout", f"the job's H does not parse as ISO-8601: {h_iso!r}") from e
    if dt.tzinfo is not None:
        dt = dt.astimezone(UTC).replace(tzinfo=None)
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


@dataclass(frozen=True)
class Holdout:
    kind: str  # "development" | "forward"
    spec: str


def plan_holdout(plan: str, h: str) -> Holdout:
    """The plan's hold-out, which is one of the two house ones (PL-6.6), else `Refused('holdout', …)`."""
    m = _HOLDOUT.search(plan)
    if not m:
        raise Refused("holdout", "the plan has no `holdout`; a development plan writes `holdout trailing:20%`, a forward plan `holdout after <H>`")
    first, second = m.group(1), m.group(2)
    if first == "after":
        if second != h:
            raise Refused("holdout", f"the forward hold-out is `holdout after {h}` with H verbatim; the plan says `after {second}`")
        return Holdout("forward", second)
    if first.startswith("trailing:"):
        w = _WINDOW.search(plan)
        if not w:
            raise Refused("holdout", f"a development plan writes `window <first bar> .. {h}`")
        if w.group(2) != h:
            raise Refused("holdout", f"the development window ends at H: `window {w.group(1)} .. {h}`; the plan says `.. {w.group(2)}`")
        return Holdout("development", first)
    raise Refused("holdout", f"`holdout {first}` is not admitted; the two hold-outs are `trailing:…` with the window ending at H and `after {h}`")


def claims_phases(plan: str) -> list[str]:
    block = _CLAIMS_BLOCK.search(plan)
    if not block:
        return []
    return [m.group(1) for m in _CLAIM_LINE.finditer(block.group(1))]


def require_after_claims(plan: str) -> None:
    """Guideline section 11: a plan without `after` claims is refused by the harness."""
    if "after" not in claims_phases(plan):
        raise Refused(
            "claims",
            "the plan has no `after` claim; the defaults are `survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after` and `not_overfit : pbo on grid sens expect statistic < 0.5 after`",
        )


def forbid_reveal(plan: str) -> None:
    if _REVEAL.search(plan):
        raise Refused("reveal", "a forward plan has no `reveal` step; reveals over (H, now] are the scheduler's")


def run_steps(plan: str) -> list[str]:
    return [m.group(1) for m in _STEP_RUN.finditer(plan)]


def has_translation_check(notes: list[str]) -> bool:
    """Guideline section 9 step 3: a note headed `Translation check` that recomputes
    a P&L to the cent and charges the commission."""
    for n in notes:
        low = n.lower()
        if "translation check" not in low:
            continue
        if "commission" in low and len(_CENTS.findall(n)) >= 2 and "=" in n:
            return True
    return False


def has_comparison_table(rationale: str) -> bool:
    """Guideline section 8: the author comparison as a table with an author column and
    an ours column and at least two measured rows."""
    lines = [ln for ln in rationale.splitlines() if _TABLE_ROW.match(ln)]
    for i, ln in enumerate(lines):
        low = ln.lower()
        if "author" in low and ("our" in low or "ours" in low or "sanity" in low):
            rows = [r for r in lines[i + 1 :] if not re.match(r"^\s*\|[\s\-:|]+\|\s*$", r)]
            if len(rows) >= 2:
                return True
    return False

"""Definitions and skills (PL-6.2, PL-6.3, contracts section 7).

A definition is one TOML file; its hash is BLAKE3 over the TOML bytes. A skill
is `SKILL.md` with a front matter (`name`, `version`); the skills hash of a job
is BLAKE3 over the canonical JSON `[[name, BLAKE3(bytes)], ...]` sorted by
name. The job directory's copy of a skill wins over the image's fallback under
`/opt/skills`.
"""

from __future__ import annotations

import json
import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

import blake3

# PL-6.4's closed set: what a definition may be granted. `provenance.write` is
# the orchestrator's own fact and is never an agent's; everything absent by
# construction (network, shell, broker, plan_reveal, pin, put) has no name here.
CLOSED_CAPS: frozenset[str] = frozenset(
    {
        "docs.read",
        "docs.search",
        "abt.catalog",
        "abt.query",
        "abt.check",
        "abt.plan",
        "abt.report",
        "abt.query_run",
        "abt.submit",
        "notes.write",
    }
)

SKILLS_FALLBACK = Path("/opt/skills")

_FRONT_MATTER = re.compile(r"\A---\n(.*?)\n---\n", re.S)


def blake3_hex(data: bytes) -> str:
    return blake3.blake3(data).hexdigest()


def canonical_json(value: object) -> str:
    """RFC 8785 for the values this module hashes: strings, numbers and lists
    of them, which serde_json's canonical form renders the same way."""
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False, sort_keys=True)


@dataclass(frozen=True)
class Model:
    provider: str
    id: str
    knowledge_cutoff: str


@dataclass(frozen=True)
class Definition:
    name: str
    stage: str
    model: Model
    skills: tuple[str, ...]
    caps: tuple[str, ...]
    budget: dict[str, float]
    records: tuple[str, ...]
    raw: bytes = field(repr=False)

    @property
    def hash(self) -> str:
        return blake3_hex(self.raw)


class DefinitionError(ValueError):
    pass


def parse_definition(raw: bytes) -> Definition:
    try:
        data = tomllib.loads(raw.decode("utf-8"))
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as e:
        raise DefinitionError(f"the definition does not parse: {e}") from e
    try:
        agent = data["agent"]
        model = agent["model"]
        d = Definition(
            name=str(agent["name"]),
            stage=str(agent["stage"]),
            model=Model(str(model["provider"]), str(model["id"]), str(model["knowledge_cutoff"])),
            skills=tuple(str(s) for s in agent["skills"]),
            caps=tuple(str(c) for c in agent["caps"]),
            budget={str(k): float(v) for k, v in data["budget"].items()},
            records=tuple(str(r) for r in data["output"]["records"]),
            raw=raw,
        )
    except KeyError as e:
        raise DefinitionError(f"the definition lacks {e}") from e
    unknown = set(d.caps) - CLOSED_CAPS
    if unknown:
        raise DefinitionError(f"caps outside PL-6.4's closed set: {sorted(unknown)}")
    return d


def load_definition(path: Path) -> Definition:
    return parse_definition(path.read_bytes())


@dataclass(frozen=True)
class Skill:
    name: str
    version: str
    body: str
    raw: bytes = field(repr=False)

    @property
    def hash(self) -> str:
        return blake3_hex(self.raw)


class SkillError(ValueError):
    pass


def parse_skill(raw: bytes, expected_name: str | None = None) -> Skill:
    text = raw.decode("utf-8")
    m = _FRONT_MATTER.match(text)
    if not m:
        raise SkillError("a skill starts with a front matter between `---` lines")
    fields: dict[str, str] = {}
    for line in m.group(1).splitlines():
        key, sep, value = line.partition(":")
        if sep:
            fields[key.strip()] = value.strip()
    if "name" not in fields or "version" not in fields:
        raise SkillError("the front matter names `name` and `version`")
    if expected_name is not None and fields["name"] != expected_name:
        raise SkillError(f"the skill calls itself {fields['name']!r}, the definition {expected_name!r}")
    return Skill(name=fields["name"], version=fields["version"], body=text[m.end() :], raw=raw)


def locate_skill(name: str, job_dir: Path, fallback: Path = SKILLS_FALLBACK) -> Path:
    """The job directory's copy wins; the image's `/opt/skills` is the fallback."""
    for root in (job_dir / "in" / "skills", fallback):
        p = root / name / "SKILL.md"
        if p.is_file():
            return p
    raise SkillError(f"skill {name!r} is neither in {job_dir / 'in' / 'skills'} nor in {fallback}")


def load_skills(names: tuple[str, ...], job_dir: Path, fallback: Path = SKILLS_FALLBACK) -> list[Skill]:
    return [parse_skill(locate_skill(n, job_dir, fallback).read_bytes(), n) for n in names]


def skills_hash(skills: list[tuple[str, bytes]]) -> str:
    """BLAKE3 over the canonical JSON `[[name, BLAKE3(bytes)], ...]` sorted by name (contracts section 7)."""
    entries = sorted([[name, blake3_hex(raw)] for name, raw in skills], key=lambda e: e[0])
    return blake3_hex(canonical_json(entries).encode("utf-8"))

"""`/job/out` (contracts section 3): records.jsonl, notes.md, cost.json, status.json."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any


def write_records(out_dir: Path, records: list[dict[str, Any]]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    with (out_dir / "records.jsonl").open("w", encoding="utf-8") as f:
        for r in records:
            f.write(json.dumps(r, ensure_ascii=False, separators=(",", ":")) + "\n")


def write_notes(out_dir: Path, notes: list[str]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    text = "\n\n".join(n.rstrip() for n in notes)
    (out_dir / "notes.md").write_text(text + ("\n" if text else ""), encoding="utf-8")


def write_cost(out_dir: Path, cost: dict[str, Any]) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "cost.json").write_text(json.dumps(cost, indent=2) + "\n", encoding="utf-8")


def write_status(out_dir: Path, state: str, reason: str = "", retryable: bool = False, detail: str = "") -> dict[str, Any]:
    out_dir.mkdir(parents=True, exist_ok=True)
    status = {"state": state, "reason": reason, "retryable": retryable, "detail": detail}
    (out_dir / "status.json").write_text(json.dumps(status, indent=2) + "\n", encoding="utf-8")
    return status

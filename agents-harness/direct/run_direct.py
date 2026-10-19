#!/usr/bin/env python3
"""The seed posts through the quant, the simplest way that works (the owner's cut
of Oct 19, 2026): no gateway, no queue, no sandbox. For each of the 50 seed
posts, the triage and the analyst's latest output are read from the earlier
campaign's job directories; the quant (`gpt-6-luna`, the house rules and its
skills as the system prompt) runs as a plain subprocess, `python -m
agents_harness` with `ABT_LOCAL=1`, so that its tools call `abt` on this
machine. The program and the plan are the agent's; the harness's local backend
holds the two rules that survive the cut (the window ends at H with `holdout
trailing:20%`; `budget { money 0 }`). Every post gets a directory under
`/srv/absolute/direct/out/<post_id>/` for the site, written as soon as each
file exists; the OpenAI spend is held under the $50 guard.

    python3 run_direct.py [--parallel 3] [--only ID,ID] [--cap-usd 50]
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import tomllib
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

S = Path("/srv/absolute")
QUEUE = S / "queue"
OUT = S / "direct" / "out"
JOBS = S / "direct" / "jobs"
P = S / "absolute-pipeline"
SKILLS = P / "agents-skills"
HARNESS = P / "agents-harness"
PY = str(S / "venv" / "bin" / "python")
SEED = S / "data" / "seed"
ABT = str(S / "absolute-backtest" / "target" / "release" / "abt")
STORE = str(S / "store")

LOCK = threading.Lock()
SPENT = {"usd": 0.0}


def now() -> str:
    return datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def jsonl(path: Path) -> list[dict[str, Any]]:
    out = []
    try:
        for line in path.read_text(encoding="utf-8").splitlines():
            try:
                out.append(json.loads(line))
            except ValueError:
                pass
    except OSError:
        pass
    return out


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def write(d: Path, name: str, value: Any) -> None:
    d.mkdir(parents=True, exist_ok=True)
    tmp = d / (name + ".tmp")
    tmp.write_text(json.dumps(value, ensure_ascii=False, indent=1, default=str), encoding="utf-8")
    tmp.replace(d / name)


def status(d: Path, state: str, stage: str, reason: str = "", **extra: Any) -> None:
    write(d, "status.json", {"state": state, "stage": stage, "reason": reason, "updated": now(), **extra})


def seed_rows() -> list[dict[str, str]]:
    rows = []
    for line in (P / "docs" / "design" / "fixtures" / "seed-posts.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or line.startswith("created_utc") or not line.strip():
            continue
        c = line.split("\t")
        rows.append({"created": c[0], "handle": c[1], "post_id": c[2], "url": c[3]})
    return rows


def staged_roots() -> list[str]:
    roots = []
    for line in (S / "logs" / "dbn" / "summary.log").read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) >= 3 and parts[1] == "exit" and parts[2] == "0":
            roots.append(parts[0])
    return sorted(set(roots))


def entity_names() -> dict[str, str]:
    """The data layer's ids and their latest symbol (equities, contracts and the continuous roots)."""
    cache = S / "direct" / "ticker.json"
    if cache.exists():
        return load(cache) or {}
    r = subprocess.run([ABT, "data", "query", "--store", STORE, "ticker"], capture_output=True, text=True, cwd="/tmp", timeout=600)
    names: dict[str, tuple[str, str]] = {}
    for line in r.stdout.splitlines():
        cells = [c.strip() for c in line.split("|")]
        if len(cells) >= 3 and cells[0].isdigit():
            i, t, sym = cells[0], cells[1], cells[2]
            if i not in names or t >= names[i][0]:
                names[i] = (t, sym)
    out = {i: s for i, (_, s) in names.items()}
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text(json.dumps(out), encoding="utf-8")
    return out


def analyst_of(doc_id: str) -> tuple[Path | None, list[dict[str, Any]], dict[str, Any]]:
    """The latest analyst job of a document: its directory, its hypotheses and its job."""
    best: tuple[float, Path] | None = None
    for jd in (QUEUE / "jobs").iterdir():
        job = load(jd / "in" / "job.json")
        if not job or job.get("stage") != "analyze" or doc_id not in (job.get("inputs") or []):
            continue
        st = jd / "out" / "status.json"
        if not st.exists():
            continue
        m = st.stat().st_mtime
        if best is None or m > best[0]:
            best = (m, jd)
    if best is None:
        return None, [], {}
    jd = best[1]
    job = load(jd / "in" / "job.json") or {}
    hyps = [r for r in jsonl(jd / "out" / "records.jsonl") if r.get("record") == "Hypothesis"]
    return jd, hyps, job


def triage_of(doc_id: str) -> dict[str, Any]:
    recs = [t for t in jsonl(QUEUE / "triage.jsonl") if t.get("doc_id") == doc_id]
    return recs[-1] if recs else {}


def assumptions_of(hyp: dict[str, Any], notes: str) -> list[str]:
    found = []
    for text in (str(hyp.get("signal") or ""), str(hyp.get("statement") or ""), notes):
        for m in re.finditer(r"[^.;\n]*\b(?:assum\w*|not stated)\b[^.;\n]*", text, re.I):
            s = m.group(0).strip(" -*•")
            if s and s not in found:
                found.append(s)
    return found[:30]


def roots_of(program: str, hyp: dict[str, Any], roots: list[str]) -> list[str]:
    named = re.findall(r"\b([A-Z0-9]{1,4})\s+from\s+\d{4}-", program)
    if not named:
        text = f"{hyp.get('universe', '')} {hyp.get('statement', '')}"
        named = [r for r in roots if re.search(rf"\b{re.escape(r)}\b", text)]
    return sorted(set(named))


def quant_job(post: dict[str, str], doc: dict[str, Any], hyp: dict[str, Any], h: str, roots: list[str], key: str) -> Path:
    jd = JOBS / post["post_id"]
    inp = jd / "in"
    (inp / "docs").mkdir(parents=True, exist_ok=True)
    (jd / "out").mkdir(parents=True, exist_ok=True)
    definition = (SKILLS / "definitions" / "quant.toml").read_bytes()
    d = tomllib.loads(definition.decode("utf-8"))
    agent = d["agent"]
    job = {
        "record": "Job",
        "id": "direct-" + post["post_id"],
        "stage": "strategize",
        "agent": agent["name"],
        "model": agent["model"]["id"],
        "caps": agent["caps"],
        "budget": d["budget"],
        "h": h,
        "knowledge_cutoff": agent["model"].get("knowledge_cutoff", ""),
        "lineage": "",
        "campaign": "direct-1",
        "inputs": [doc["id"], hyp.get("id", "")],
    }
    (inp / "job.json").write_text(json.dumps(job, indent=1), encoding="utf-8")
    (inp / "token").write_text("local\n", encoding="utf-8")
    (inp / "definition.toml").write_bytes(definition)
    for s in agent["skills"]:
        (inp / "skills" / s).mkdir(parents=True, exist_ok=True)
        shutil.copy(SKILLS / "skills" / s / "SKILL.md", inp / "skills" / s / "SKILL.md")
    (inp / "docs" / f"{doc['id']}.json").write_text(json.dumps(doc, ensure_ascii=False, indent=1), encoding="utf-8")
    (inp / "docs" / "closure.json").write_text(json.dumps({"inputs": [doc["id"]], "closure": [doc["id"]]}), encoding="utf-8")
    (inp / "hypothesis.json").write_text(json.dumps(hyp, ensure_ascii=False, indent=1), encoding="utf-8")
    return jd


def run_quant(jd: Path, roots: list[str], key: str, wall_s: float) -> int | None:
    if (jd / "out" / "status.json").exists():
        return 0
    env = dict(os.environ)
    env.update(
        {
            "JOB_DIR": str(jd),
            "ABT_LOCAL": "1",
            "ABT_ROOTS": ",".join(roots),
            "ABT_BIN": ABT,
            "ABT_STORE": STORE,
            "ABT_DATA": str(S / "run" / "data.sock"),
            "OPENAI_API_KEY": key,
            "OPENAI_BASE_URL": "https://api.openai.com/v1",
            "PIPELINE_JOB_ID": jd.name,
        }
    )
    with (jd / "harness.log").open("w", encoding="utf-8") as log:
        try:
            p = subprocess.run([PY, "-m", "agents_harness"], cwd=HARNESS, env=env, stdout=log, stderr=log, timeout=wall_s)
            return p.returncode
        except subprocess.TimeoutExpired:
            (jd / "out" / "status.json").write_text(json.dumps({"state": "failed", "reason": "budget", "retryable": False, "detail": f"wall time over {wall_s:.0f} s"}), encoding="utf-8")
            return 2


def collect(post: dict[str, str], d: Path, jd: Path, hyp: dict[str, Any], h: str, roots: list[str], names: dict[str, str]) -> None:
    out = jd / "out"
    st = load(out / "status.json") or {}
    notes = (out / "notes.md").read_text(encoding="utf-8") if (out / "notes.md").exists() else ""
    cost = load(out / "cost.json") or {}
    work = out / "abt"
    last = load(work / "last_plan.json") or {}
    sub = load(work / "submission.json") or {}
    recs = [r for r in jsonl(out / "records.jsonl") if r.get("record") == "Submission"]
    programs = last.get("programs") or {}
    program = next(iter(programs.values()), "") if programs else ""
    if not program:
        # The last program the checker saw, when no plan ran.
        calls = jsonl(work / "calls.jsonl")
        checks = [c for c in calls if c.get("route") == "check"]
        if checks:
            program = str(checks[-1]["body"].get("program", ""))
    plan_run = (last.get("report") or {}).get("plan_run") or {}
    reveal = (last.get("report") or {}).get("reveal")
    if plan_run:
        write(d, "report.json", plan_run)
    if isinstance(reveal, dict) and reveal.get("outcomes"):
        write(d, "reveal.json", reveal)
    tables = (last.get("report") or {}).get("tables")
    if tables:
        write(d, "plan_report.json", tables)
    ph = str(last.get("plan_hash") or "")[:16]
    for label in ("reveal", "base"):
        nav = work / f"nav-{label}-{ph}.csv"
        if ph and nav.exists() and nav.stat().st_size > 10:
            shutil.copy(nav, d / "nav.csv")
            break
    ents: dict[str, str] = {}
    for o in (plan_run.get("outcomes") or []) + ((reveal or {}).get("outcomes") or [] if isinstance(reveal, dict) else []):
        for e in (o.get("trades") or {}).get("by_entity") or []:
            k = str(e.get("entity"))
            if k in names:
                ents[k] = names[k]
    lineage = ""
    if plan_run.get("program_hash"):
        lineage = str(plan_run.get("lineage") or "")
    full = any(o.get("step") == "base" and o.get("status") == "completed" for o in plan_run.get("outcomes") or [])
    rationale = str(sub.get("rationale") or (recs[0].get("rationale") if recs else "") or "")
    quant = {
        "state": st.get("state"),
        "reason": st.get("reason") or ("" if full else "no development plan ran"),
        "detail": st.get("detail"),
        "h": h,
        "roots": roots_of(program, hyp, roots),
        "assumptions": assumptions_of(hyp, notes),
        "program_text": program,
        "plan_text": last.get("plan") or "",
        "program_hash": plan_run.get("program_hash") or "",
        "plan_hash": last.get("plan_hash") or "",
        "lineage": lineage,
        "trials": plan_run.get("trials"),
        "trial_kinds": plan_run.get("trial_kinds"),
        "entity_names": ents,
        "notes": notes[-20_000:],
        "cost_usd": cost.get("money_usd"),
        "submitted": bool(sub or recs),
        "full_backtest": full,
    }
    write(d, "quant.json", quant)
    verdict = {
        "rationale": rationale,
        "verdict": (st.get("detail") or "") if not rationale else "",
        "author_reported": hyp.get("author_reported"),
    }
    write(d, "verdict.json", verdict)
    if full:
        status(d, "done", "quant", "" if st.get("state") == "done" else f"{st.get('reason')}: {st.get('detail') or ''}"[:600], full_backtest=True)
    else:
        status(d, "failed", "quant", f"{st.get('reason') or 'failed'}: {st.get('detail') or ''}"[:600], full_backtest=False)


def spent_so_far() -> float:
    base = sum(float(j.get("cost_usd") or 0) for j in jsonl(QUEUE / "jobs.jsonl"))
    direct = 0.0
    if JOBS.exists():
        for jd in JOBS.iterdir():
            c = load(jd / "out" / "cost.json") or {}
            direct += float(c.get("money_usd") or 0)
    return base + direct


def one(post: dict[str, str], doc: dict[str, Any], roots: list[str], names: dict[str, str], key: str, cap: float) -> str:
    d = OUT / post["post_id"]
    mirror = load(SEED / f"{post['post_id']}.json") or {}
    tw = mirror.get("tweet") or {}
    images = []
    for m in (tw.get("media") or {}).get("all") or []:
        u = m.get("url") if m.get("type") == "photo" else (m.get("thumbnail_url") or m.get("url"))
        if u:
            images.append(u)
    tweet = dict(doc)
    tweet["author"] = {**(doc.get("author") or {}), "name": (tw.get("author") or {}).get("name"), "screen_name": (tw.get("author") or {}).get("screen_name")}
    tweet["images"] = images
    tweet["url"] = post["url"]
    write(d, "tweet.json", tweet)
    write(d, "triage.json", triage_of(doc["id"]))
    jd_a, hyps, ajob = analyst_of(doc["id"])
    if not hyps:
        notes = (jd_a / "out" / "notes.md").read_text(encoding="utf-8") if jd_a and (jd_a / "out" / "notes.md").exists() else ""
        reason = notes.strip().split("\n\n")[-1][:1200] if notes.strip() else "the analyst found no falsifiable hypothesis"
        write(d, "analysis.json", {"testable": False, "reason": reason, "notes": notes[-8000:], "job": jd_a.name if jd_a else None})
        status(d, "failed", "analyst", "not testable: " + reason[:500])
        return "no_hypothesis"
    hyp = max(hyps, key=lambda x: float(x.get("confidence") or 0))
    h = str(ajob.get("h") or hyp.get("h") or "")
    hyp = {**hyp, "h": h, "id": hyp.get("id") or f"hyp-{post['post_id']}", "doc_ids": [doc["id"]]}
    write(d, "hypothesis.json", hyp)
    with LOCK:
        SPENT["usd"] = spent_so_far()
        if SPENT["usd"] + 0.25 > cap:
            status(d, "failed", "quant", f"the OpenAI guard: ${SPENT['usd']:.2f} spent of ${cap:.2f}")
            return "budget"
    status(d, "running", "quant")
    jd = quant_job(post, doc, hyp, h, roots, key)
    budget = tomllib.loads((SKILLS / "definitions" / "quant.toml").read_text(encoding="utf-8"))["budget"]
    t0 = time.time()
    code = run_quant(jd, roots, key, float(budget.get("wall_s", 1500)) + 600)
    collect(post, d, jd, hyp, h, roots, names)
    st = load(d / "status.json") or {}
    print(f"{now()} post {post['post_id']} @{post['handle']}: {st.get('state')} {st.get('reason', '')[:120]} (exit {code}, {time.time() - t0:.0f} s)", flush=True)
    return str(st.get("state"))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--parallel", type=int, default=3)
    ap.add_argument("--only", default="")
    ap.add_argument("--cap-usd", type=float, default=50.0)
    a = ap.parse_args()
    key = Path("/etc/absolute-pipeline/openai_api_key").read_text(encoding="utf-8").strip()
    docs = {d["source_id"]: d for d in jsonl(QUEUE / "docs.jsonl")}
    roots = staged_roots()
    names = entity_names()
    posts = [p for p in seed_rows() if not a.only or p["post_id"] in a.only.split(",")]
    order = []
    for p in posts:
        doc = docs.get(p["post_id"])
        if doc is None:
            d = OUT / p["post_id"]
            write(d, "tweet.json", {"source_id": p["post_id"], "author": {"handle": p["handle"]}, "fixed_at": p["created"], "url": p["url"]})
            status(d, "failed", "fetch", "the post was not fetched")
            continue
        _, hyps, _ = analyst_of(doc["id"])
        conf = max((float(h.get("confidence") or 0) for h in hyps), default=-1.0)
        order.append((-conf, p, doc))
        if not (OUT / p["post_id"] / "status.json").exists():
            status(OUT / p["post_id"], "queued", "quant" if hyps else "analyst")
    order.sort(key=lambda x: x[0])
    print(f"{now()} {len(order)} posts; {sum(1 for o in order if o[0] <= 0)} with a hypothesis; roots {','.join(roots)}; spent ${spent_so_far():.2f}", flush=True)
    results: dict[str, int] = {}
    # The posts without a hypothesis get their page at once (no model call).
    for _neg, p, doc in [o for o in order if o[0] > 0]:
        try:
            r = one(p, doc, roots, names, key, a.cap_usd)
        except Exception as e:  # noqa: BLE001
            r = f"error: {type(e).__name__}: {e}"
        results[r] = results.get(r, 0) + 1
    order = [o for o in order if o[0] <= 0]
    with ThreadPoolExecutor(max_workers=max(1, a.parallel)) as ex:
        futs = [ex.submit(one, p, doc, roots, names, key, a.cap_usd) for _, p, doc in order]
        for f in futs:
            try:
                r = f.result()
            except Exception as e:  # noqa: BLE001 - one post's failure never stops the run
                r = f"error: {type(e).__name__}: {e}"
                print(f"{now()} {r}", flush=True)
            results[r] = results.get(r, 0) + 1
    full = sum(1 for p in posts if (load(OUT / p["post_id"] / "status.json") or {}).get("full_backtest"))
    print(f"{now()} done: {results}; full backtests {full} of {len(posts)}; OpenAI spend ${spent_so_far():.2f}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())

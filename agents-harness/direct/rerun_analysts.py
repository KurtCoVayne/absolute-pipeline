#!/usr/bin/env python3
"""The seed posts the analyst found nothing in, read again with what it lacked:
the post's images (the mirror's photos and video thumbnails), the page or X
article a link-only post points to, and the store's data map (the futures
roots held, the equity universe, what is not held, and the symbol mapping).

For each post whose analyst stopped with "not testable", triage and the
analyst run again as plain subprocesses (`python -m agents_harness`,
`ABT_LOCAL=1` so `resolve_symbols` and `catalog` answer from the store), in
job directories under /srv/absolute/direct/jobs-analyst/, each linked into
/srv/absolute/queue/jobs/ so the quant run (`run_direct.py`) reads the newest
analyst output of a post as it always has. A post that now has a hypothesis
gets `hypothesis.json` and `status.json = queued`; the others an updated
`analysis.json` with the new reason. The OpenAI spend stays under the $50
guard (every job's cost is also appended to queue/jobs.jsonl, which the guard
sums); an X article read through xAI is priced into the X guard ($25).

    python3 rerun_analysts.py [--only ID,ID] [--parallel 6] [--dry-run]
"""

from __future__ import annotations

import argparse
import hashlib
import html
import ipaddress
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import threading
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

S = Path(os.environ.get("ABT_ROOT", "/srv/absolute"))
QUEUE = S / "queue"
OUT = S / "direct" / "out"
JOBS = S / "direct" / "jobs-analyst"
P = S / "absolute-pipeline"
SKILLS = P / "agents-skills"
HARNESS = P / "agents-harness"
PY = str(S / "venv" / "bin" / "python")
SEED = S / "data" / "seed"
ABT = str(S / "absolute-backtest" / "target" / "release" / "abt")
STORE = str(S / "store")
USER_AGENT = "absolute-pipeline/0.1 (research, read-only)"
MAX_LINK_BYTES = 2 * 1024 * 1024
MAX_LINK_CHARS = 40_000
X_CAP_USD = 25.0
LOCK = threading.Lock()

# What the store does not hold, whatever a post needs (house rules section 2).
NOT_HELD = [
    "options and implied volatility",
    "CFTC Commitments of Traders positioning",
    "market breadth (advance/decline, new highs/lows)",
    "fundamentals, earnings dates, analyst estimates",
    "crypto spot prices, on-chain data, altcoin indexes (only the CME BTC, ETH and SOL futures, when listed below)",
    "non-US indexes and their futures unless a root below is one (no Euro Stoxx, DAX, FTSE)",
    "single stocks outside the S&P 500 since 2014",
    "macro series (rates curves, PMIs, money supply) beyond the rate futures listed",
]

MAPPING = [
    ("QQQ, NDX, Nasdaq 100, NQ1!", "NQ (E-mini Nasdaq-100 future); MNQ for the micro"),
    ("SPY, SPX, S&P 500, ES1!", "ES (E-mini S&P 500 future); MES for the micro"),
    ("IWM, Russell 2000", "RTY"),
    ("bitcoin, BTC, BTCUSD", "BTC (the CME Bitcoin future)"),
    ("ether, ETH, ETHUSD", "ETH (the CME Ether future)"),
    ("solana, SOL, SOLUSD", "SOL (the CME Solana future), when listed below"),
    ("gold, GLD, XAU", "GC when listed below"),
    ("crude oil, WTI, USO", "CL when listed below"),
    ("10-year Treasury, TLT", "ZN when listed below; ZB and UB for the long end"),
    ("a stock ticker (NVDA, COIN, …)", "the equity of that ticker when it was an S&P 500 member at the time (check with resolve_symbols)"),
]


def now() -> str:
    return datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def load(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


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


def write(d: Path, name: str, value: Any) -> None:
    d.mkdir(parents=True, exist_ok=True)
    tmp = d / (name + ".tmp")
    tmp.write_text(json.dumps(value, ensure_ascii=False, indent=1, default=str), encoding="utf-8")
    tmp.replace(d / name)


def append_line(path: Path, value: dict[str, Any]) -> None:
    with LOCK, path.open("a", encoding="utf-8") as f:
        f.write(json.dumps(value, ensure_ascii=False) + "\n")


# ---------------------------------------------------------------- the data map


def parse_rows(text: str) -> list[list[str]]:
    """`abt data query`'s table: the request line, a header `a | b | …`, then rows."""
    rows = []
    for line in text.splitlines():
        cells = [c.strip() for c in line.split("|")]
        if len(cells) >= 2 and cells[0] and cells[0][0].isdigit():
            rows.append(cells)
    return rows


def roots_from_rows(continuous: list[list[str]], tickers: list[list[str]]) -> list[tuple[str, str]]:
    """The staged futures roots: the `continuous` root ids named by their
    `ticker` symbol, each with the date of its first bar (the ticker row's t)."""
    ids = {r[0] for r in continuous}
    named: dict[str, tuple[str, str]] = {}
    for r in tickers:
        if len(r) >= 3 and r[0] in ids:
            sym, t = r[2], r[1][:10]
            if sym and (sym not in named or t < named[sym][1]):
                named[sym] = (sym, t)
    return sorted(named.values())


def render_data_map(roots: list[tuple[str, str]], *, equity_span: str = "2014-01-02 .. 2026-10-14", equity_names: int = 756) -> str:
    lines = [
        "# The store's data map (what the backtester can test on)",
        "",
        f"**Futures** ({len(roots)} roots staged; back-adjusted continuous series per root, minute bars rolled up to sessions and days, from the date shown to 2026-08-14):",
        " ".join(f"{sym} ({first})" for sym, first in roots),
        "",
        f"**Equities**: the S&P 500's members, current and past, {equity_names} names, daily bars {equity_span}, with splits, dividends, delistings, "
        "index membership by day, and GICS sector, industry group, industry and sub-industry from one snapshot dated 2026-09-21 (no rows before it).",
        "",
        "**Not held**: " + "; ".join(NOT_HELD) + ".",
        "",
        "**Mapping the author's instrument** (say which you chose in `universe`):",
    ]
    lines += [f"- {k} → {v}" for k, v in MAPPING]
    lines += ["", "A market listed here is held: never call it outside the data. A market not listed is not held: say so and name what is missing."]
    return "\n".join(lines)


def build_data_map(h: str) -> tuple[str, list[str]]:
    q = lambda text: subprocess.run([ABT, "data", "query", "--store", STORE, text], capture_output=True, text=True, cwd="/tmp", timeout=600).stdout  # noqa: E731
    roots = roots_from_rows(parse_rows(q(f"continuous latest at {h}")), parse_rows(q("ticker")))
    return render_data_map(roots), [r[0] for r in roots]


# ---------------------------------------------------------------- images


CHART_WORDS = re.compile(r"\b(chart|backtest|equity|sharpe|cagr|drawdown|return|win rate|seasonality|table|results?|performance|%|signal|setup|pattern|breakout)\b", re.I)


def images_of(mirror: dict[str, Any], text: str, limit: int = 4) -> list[dict[str, str]]:
    """The post's photos and video thumbnails, at most `limit`; `high` detail
    when the image is likely a chart with numbers (the post talks about one, or
    its text is too short to carry the idea), `low` otherwise."""
    media = ((mirror.get("tweet") or {}).get("media") or {}).get("all") or []
    words = re.sub(r"https?://\S+", "", text).strip()
    chart = bool(CHART_WORDS.search(text)) or len(words) < 160
    out = []
    for m in media:
        kind = str(m.get("type") or "")
        url = str(m.get("url") if kind == "photo" else (m.get("thumbnail_url") or ""))
        if not url.startswith("https://"):
            continue
        out.append({"url": url, "kind": "photo" if kind == "photo" else f"{kind} thumbnail", "detail": "high" if chart and kind == "photo" else "low"})
        if len(out) >= limit:
            break
    return out


# ---------------------------------------------------------------- links


def is_link_only(text: str) -> bool:
    return bool(re.search(r"https?://", text)) and len(re.sub(r"https?://\S+", "", text).strip()) < 60


def public_host(host: str) -> bool:
    """Every address the host resolves to is public (no private, loopback,
    link-local, reserved or multicast range: the SSRF rule of PL-5.5)."""
    try:
        infos = socket.getaddrinfo(host, None)
    except OSError:
        return False
    for info in infos:
        ip = ipaddress.ip_address(info[4][0])
        if ip.is_private or ip.is_loopback or ip.is_link_local or ip.is_reserved or ip.is_multicast or ip.is_unspecified:
            return False
    return bool(infos)


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a: Any, **k: Any) -> None:  # noqa: ANN401
        return None


def http_text(url: str, *, timeout: float = 10.0, hops: int = 3) -> str:
    """A public page's text: HTTPS only, each hop's host checked public, at most
    `hops` redirects, 2 MB, text/html or text/plain, tags and scripts stripped."""
    opener = urllib.request.build_opener(_NoRedirect)
    for _ in range(hops + 1):
        u = urllib.parse.urlsplit(url)
        if u.scheme != "https" or not u.hostname or u.username or not public_host(u.hostname):
            raise ValueError(f"refused: {url}")
        req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "text/html,text/plain"})
        try:
            with opener.open(req, timeout=timeout) as r:
                ctype = r.headers.get("Content-Type", "")
                if not ctype.startswith(("text/html", "text/plain")):
                    raise ValueError(f"refused content type {ctype!r}")
                body = r.read(MAX_LINK_BYTES + 1)[:MAX_LINK_BYTES].decode("utf-8", "replace")
                return html_to_text(body) if "html" in ctype else body
        except urllib.error.HTTPError as e:
            if e.code in (301, 302, 303, 307, 308) and e.headers.get("Location"):
                url = urllib.parse.urljoin(url, e.headers["Location"])
                continue
            raise
    raise ValueError("too many redirects")


def html_to_text(body: str) -> str:
    body = re.sub(r"(?is)<(script|style|noscript|svg|nav|footer|header)\b.*?</\1>", " ", body)
    body = re.sub(r"(?i)<br\s*/?>|</p>|</h\d>|</li>", "\n", body)
    body = re.sub(r"(?s)<[^>]+>", " ", body)
    body = html.unescape(body)
    return re.sub(r"[ \t]+", " ", re.sub(r"\n\s*\n+", "\n\n", body)).strip()


def x_article_text(url: str, post_url: str, key: str) -> tuple[str, float]:
    """An X article's text through xAI's X search (PL-D9), and its cost."""
    body = {
        "model": "grok-4.20-non-reasoning",
        "input": f"Return the full text of the X article {url}, linked from the post {post_url}, verbatim (title, then body; tables as text). Output only the article's text. If you cannot read it, output exactly NOT_AVAILABLE.",
        "tools": [{"type": "x_search"}],
        "max_tool_calls": 6,
    }
    req = urllib.request.Request("https://api.x.ai/v1/responses", data=json.dumps(body).encode(), headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=300) as r:
        v = json.loads(r.read())
    cost = float((v.get("usage") or {}).get("cost_in_usd_ticks") or 0) * 1e-10
    text = "\n".join(str(c.get("text", "")) for o in v.get("output") or [] if o.get("type") == "message" for c in o.get("content") or [])
    return ("" if "NOT_AVAILABLE" in text else text.strip()), cost


def x_spent() -> float:
    return 0.40 + sum(float(s.get("cost_usd") or 0) for s in jsonl(QUEUE / "spend.jsonl"))


def linked_document(doc: dict[str, Any], post_url: str, xai_key: str | None, *, dry: bool = False) -> dict[str, Any] | None:
    """The page a link-only post points to, as a document of its own (its
    `fixed_at` is the post's: the seed campaign reads every input as of the
    post, the owner's rule; the fetch time is `r`)."""
    text = str(doc.get("text", ""))
    if not is_link_only(text):
        return None
    urls = [u for u in re.findall(r"https?://\S+", text)]
    if not urls:
        return None
    url = urls[0].rstrip(").,")
    cache = JOBS / "links" / f"{doc.get('source_id')}.json"
    cached = load(cache)
    if cached:
        return cached
    if dry:
        return {"error": "dry run: not fetched", "url": url}
    got, via, cost = "", "", 0.0
    try:
        m = re.match(r"https?://(?:x|twitter)\.com/(\w+)/status/(\d+)", url)
        if m:
            req = urllib.request.Request(f"https://api.fxtwitter.com/{m.group(1)}/status/{m.group(2)}", headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=10) as r:
                t = json.loads(r.read()).get("tweet") or {}
            got, via = str(t.get("text") or ""), "mirror"
        elif re.match(r"https?://(?:x|twitter)\.com/i/article/\d+", url):
            if xai_key and x_spent() + 1.0 <= X_CAP_USD:
                got, cost = x_article_text(url, post_url, xai_key)
                via = "xai"
                append_line(QUEUE / "spend.jsonl", {"service": "xai", "what": f"article {url}", "cost_usd": cost, "r": now()})
        else:
            got, via = http_text(url), "https"
    except (OSError, ValueError, urllib.error.URLError) as e:
        return {"error": f"{type(e).__name__}: {e}", "url": url}
    if not got.strip():
        return {"error": "empty", "url": url, "via": via}
    result = _linked(doc, url, got, via, cost)
    write(cache.parent, cache.name, result)
    return result


def _linked(doc: dict[str, Any], url: str, got: str, via: str, cost: float) -> dict[str, Any]:
    got = got[:MAX_LINK_CHARS]
    r = now()
    return {
        "record": "Document",
        "id": hashlib.blake2b(f"{url}\n{got}".encode(), digest_size=32).hexdigest(),
        "version": 1,
        "source": "X" if via in ("mirror", "xai") else "Web",
        "author": doc.get("author") or {},
        "p": doc.get("fixed_at"),
        "fixed_at": doc.get("fixed_at"),
        "r": r,
        "r_n": r,
        "parents": [{"kind": "Link", "doc_id": doc.get("id"), "source_id": doc.get("source_id")}],
        "text": got,
        "links": [],
        "attachments": [{"kind": "url", "url": url}],
        "via": via,
        "cost_usd": cost,
    }


# ---------------------------------------------------------------- jobs


def spent_openai() -> float:
    seen: set[Path] = set()
    total = sum(float(j.get("cost_usd") or 0) for j in jsonl(QUEUE / "jobs.jsonl"))
    for jd in (S / "direct" / "jobs").glob("*"):
        rp = jd.resolve()
        if rp not in seen:
            seen.add(rp)
            total += float((load(jd / "out" / "cost.json") or {}).get("money_usd") or 0)
    return total


def job_dir(stage: str, agent: str, doc: dict[str, Any], docs: list[dict[str, Any]], images: list[dict[str, str]], h: str, data_map: str) -> Path:
    definition = (SKILLS / "definitions" / f"{agent}.toml").read_bytes()
    d = tomllib.loads(definition.decode("utf-8"))["agent"]
    skills_digest = hashlib.blake2b(b"".join((SKILLS / "skills" / s / "SKILL.md").read_bytes() for s in d["skills"]), digest_size=16).hexdigest()
    ident = hashlib.blake2b(json.dumps([stage, doc["id"], [x["id"] for x in docs], hashlib.blake2b(definition).hexdigest(), skills_digest, data_map]).encode(), digest_size=32).hexdigest()
    jd = JOBS / ident
    if (jd / "out" / "status.json").exists():
        return jd
    shutil.rmtree(jd, ignore_errors=True)
    inp = jd / "in"
    (inp / "docs").mkdir(parents=True)
    (jd / "out").mkdir()
    budget = tomllib.loads(definition.decode("utf-8"))["budget"]
    job = {
        "record": "Job",
        "id": ident,
        "stage": stage,
        "agent": agent,
        "model": d["model"]["id"],
        "caps": d["caps"],
        "budget": budget,
        "h": h,
        "knowledge_cutoff": d["model"]["knowledge_cutoff"],
        "lineage": "",
        "campaign": "seed-1-reread",
        "inputs": [doc["id"]],
    }
    (inp / "job.json").write_text(json.dumps(job, indent=1), encoding="utf-8")
    (inp / "token").write_text("local\n", encoding="utf-8")
    (inp / "definition.toml").write_bytes(definition)
    for s in d["skills"]:
        (inp / "skills" / s).mkdir(parents=True)
        shutil.copy(SKILLS / "skills" / s / "SKILL.md", inp / "skills" / s / "SKILL.md")
    for x in docs:
        (inp / "docs" / f"{x['id']}.json").write_text(json.dumps(x, ensure_ascii=False, indent=1), encoding="utf-8")
    if images:
        (inp / "docs" / f"{doc['id']}.images.json").write_text(json.dumps(images, indent=1), encoding="utf-8")
    (inp / "docs" / "closure.json").write_text(json.dumps({"inputs": [doc["id"]], "closure": [x["id"] for x in docs]}), encoding="utf-8")
    (inp / "data_map.md").write_text(data_map, encoding="utf-8")
    return jd


def run_job(jd: Path, roots: list[str], key: str) -> None:
    if (jd / "out" / "status.json").exists():
        return
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
            subprocess.run([PY, "-m", "agents_harness"], cwd=HARNESS, env=env, stdout=log, stderr=log, timeout=600)
        except subprocess.TimeoutExpired:
            write(jd / "out", "status.json", {"state": "failed", "reason": "budget", "detail": "wall time over 600 s"})
    cost = float((load(jd / "out" / "cost.json") or {}).get("money_usd") or 0)
    job = load(jd / "in" / "job.json") or {}
    append_line(QUEUE / "jobs.jsonl", {"record": "Job", "id": jd.name, "stage": job.get("stage"), "agent": job.get("agent"), "cost_usd": cost, "campaign": "seed-1-reread", "r": now()})


def reread(post_id: str, doc: dict[str, Any], h: str, data_map: str, roots: list[str], key: str, xai_key: str | None, dry: bool) -> str:
    d = OUT / post_id
    mirror = load(SEED / f"{post_id}.json") or {}
    text = str(doc.get("text", ""))
    images = images_of(mirror, text)
    post_url = str(((load(d / "tweet.json") or {}).get("url")) or f"https://x.com/i/status/{post_id}")
    linked = linked_document(doc, post_url, xai_key, dry=dry)
    docs = [doc] + ([linked] if linked and "error" not in linked else [])
    note = {"images": len(images), "linked": (linked or {}).get("via") or (linked or {}).get("error")}
    if dry:
        return f"dry: {note}"
    if spent_openai() + 0.06 > 50.0:
        return "budget: the OpenAI guard"
    tj = job_dir("triage", "triage", doc, docs, images, h, data_map)
    aj = job_dir("analyze", "analyst", doc, docs, images, h, data_map)
    run_job(tj, roots, key)
    run_job(aj, roots, key)
    triage = next((r for r in jsonl(tj / "out" / "records.jsonl") if r.get("record") == "Triage"), None)
    if triage:
        write(d, "triage.json", {**triage, "doc_id": doc["id"], "job_id": tj.name, "reread": True})
    # The quant run reads a post's newest analyst job from queue/jobs.
    link = QUEUE / "jobs" / aj.name
    if not link.exists():
        link.symlink_to(aj)
    hyps = [r for r in jsonl(aj / "out" / "records.jsonl") if r.get("record") == "Hypothesis"]
    notes = (aj / "out" / "notes.md").read_text(encoding="utf-8") if (aj / "out" / "notes.md").exists() else ""
    st = load(aj / "out" / "status.json") or {}
    if hyps:
        hyp = max(hyps, key=lambda x: float(x.get("confidence") or 0))
        hyp = {**hyp, "h": h, "id": hyp.get("id") or f"hyp-{post_id}", "doc_ids": [doc["id"]]}
        write(d, "hypothesis.json", hyp)
        write(d, "status.json", {"state": "queued", "stage": "quant", "reason": "", "updated": now(), "reread": note})
        return f"hypothesis ({float(hyp.get('confidence') or 0):.2f}): {str(hyp.get('statement'))[:160]}"
    reason = notes.strip().split("\n\n")[-1][:1200] if notes.strip() else (st.get("detail") or "the analyst found no falsifiable hypothesis")
    write(d, "analysis.json", {"testable": False, "reason": reason, "notes": notes[-8000:], "job": aj.name, "reread": note})
    write(d, "status.json", {"state": "failed", "stage": "analyst", "reason": "not testable: " + reason[:500], "updated": now(), "reread": note})
    return f"no hypothesis: {reason[:160]}"


def candidates(only: set[str]) -> list[str]:
    out = []
    for d in sorted(OUT.iterdir()):
        st = load(d / "status.json") or {}
        if only and d.name not in only:
            continue
        if st.get("stage") == "analyst" and str(st.get("reason", "")).startswith("not testable"):
            out.append(d.name)
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default="")
    ap.add_argument("--parallel", type=int, default=6)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    key = Path("/etc/absolute-pipeline/openai_api_key").read_text(encoding="utf-8").strip()
    xk = Path("/etc/absolute-pipeline/xai_api_key")
    xai_key = xk.read_text(encoding="utf-8").strip() if xk.exists() else None
    docs = {d["source_id"]: d for d in jsonl(QUEUE / "docs.jsonl")}
    posts = candidates({p for p in a.only.split(",") if p})
    JOBS.mkdir(parents=True, exist_ok=True)
    data_map, roots = build_data_map(now())
    (JOBS / "data_map.md").write_text(data_map, encoding="utf-8")
    print(f"{now()} {len(posts)} posts to reread; {len(roots)} roots: {','.join(roots)}; OpenAI spent ${spent_openai():.2f}", flush=True)
    h_of = {}
    for pid in posts:
        for jd in (QUEUE / "jobs").iterdir():
            job = load(jd / "in" / "job.json") or {}
            if job.get("stage") == "analyze" and docs.get(pid, {}).get("id") in (job.get("inputs") or []):
                h_of[pid] = str(job.get("h") or "")
    with ThreadPoolExecutor(max_workers=max(1, a.parallel)) as ex:
        futs = {pid: ex.submit(reread, pid, docs[pid], h_of.get(pid) or str(docs[pid].get("fixed_at")), data_map, roots, key, xai_key, a.dry_run) for pid in posts if pid in docs}
        for pid, f in futs.items():
            try:
                r = f.result()
            except Exception as e:  # noqa: BLE001 - one post's failure never stops the rest
                r = f"error: {type(e).__name__}: {e}"
            print(f"{now()} {pid}: {r}", flush=True)
    print(f"{now()} done; OpenAI spent ${spent_openai():.2f}; X spent ${x_spent():.2f}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())

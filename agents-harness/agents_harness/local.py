"""The quant's tools against `abt` on the machine itself (the owner's cut of Oct 19:
no gateway, no queue, no sandbox). `ABT_LOCAL=1` makes `python -m agents_harness`
use `LocalAbt` in place of the HTTP gateway: the same route names and result
shapes (contracts section 5), each answered by the `abt` command line.

Two rules survive the cut and are enforced here before `abt` sees a plan: the
plan's `window` ends at the job's H with `holdout trailing:20%`, and its
`budget` is `money 0`. The window's start is never limited (the owner: as much
history as the store holds). Data queries read `r <= H`.

Environment: `ABT_BIN` (the `abt` binary), `ABT_LIB` (the library sources, a
glob or a space-separated list), `ABT_DATA` (the data service's socket),
`ABT_DATA_DAILY` (Norgate's daily continuous futures, for programs on `<ROOT>_N`),
`ABT_STORE` (the store, read-only reads), `ABT_WORK` (where programs, plans and
answers are written), `ABT_ROOTS` (the futures roots staged, comma-separated).
"""

from __future__ import annotations

import glob
import hashlib
import json
import os
import re
import subprocess
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from .gateway import GatewayError

MAX_ROWS = 500
MAX_TEXT = 8_000
_WINDOW = re.compile(r"^(\s*window\s+)(\S+)(\s*\.\.\s*)(\S+)", re.M)
_HOLDOUT = re.compile(r"^\s*holdout\s+(\S+)", re.M)
_BUDGET = re.compile(r"^\s*budget\s*\{([^}]*)\}", re.M)
_PROGRAM = re.compile(r"^\s*program\s+(\S+)", re.M)
_STRATEGY = re.compile(r"^\s*strategy\s+([A-Za-z_][A-Za-z0-9_]*)", re.M)
_NORGATE_ROOT = re.compile(r"\b[A-Z][A-Z0-9]*_N\b")
_DIAG = re.compile(r"^(error|warning)\s+\[([A-Za-z0-9-]+)\]\s+(.*)$")


def _iso(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def _parse_time(s: str) -> datetime:
    s = s.strip().rstrip("Z")
    if "." in s:
        s = s.split(".", 1)[0]
    dt = datetime.fromisoformat(s)
    if dt.tzinfo is not None:
        dt = dt.astimezone(UTC).replace(tzinfo=None)
    return dt


def parse_table(text: str, max_rows: int = MAX_ROWS) -> dict[str, Any]:
    """`abt data query`'s answer (a request line, then `a | b | c` rows) as
    `{columns, rows, truncated}`."""
    lines = [ln for ln in text.splitlines() if " | " in ln or ln.strip().startswith("row |")]
    if not lines:
        return {"columns": [], "rows": [], "truncated": False, "text": text[:MAX_TEXT]}
    cols = [c.strip() for c in lines[0].split("|")]
    rows: list[list[Any]] = []
    for ln in lines[1:]:
        cells = [c.strip() for c in ln.split("|")]
        rows.append([_num(c) for c in cells])
    if cols and cols[0] == "row":
        cols = cols[1:]
        rows = [r[1:] for r in rows]
    return {"columns": cols, "rows": rows[:max_rows], "truncated": len(rows) > max_rows}


def _num(c: str) -> Any:
    if c == "":
        return None
    try:
        if re.fullmatch(r"-?\d+", c):
            return int(c)
        return float(c) if re.fullmatch(r"-?\d+(\.\d+)?([eE][-+]?\d+)?", c) else c
    except ValueError:
        return c


def reads_minutes(program: str) -> bool:
    """Whether a program evaluates on minute bars directly: an intraday decision clock
    (`decide on base` on the futures class, `at session + …`), a sub-session interval
    (`session(X, [0min, …))`), or a rule on an `@1m` key. A daily program decides on
    sessions (or on an equity's daily base) and reads bars through `@session` views."""
    body = re.sub(r"data\s*\{[^}]*\}", "", program)
    if re.search(r"decide\s+on\s+base\s+at\b", body) or re.search(r"session\s*\(\s*\w+\s*,\s*\[", body) or "@1m" in body:
        return True
    return bool(re.search(r"class\s+future", program) and re.search(r"decide\s+on\s+base\b", body))


def available_gb() -> float:
    """MemAvailable from /proc/meminfo, in GB (a large number off Linux)."""
    try:
        for ln in Path("/proc/meminfo").read_text(encoding="utf-8").splitlines():
            if ln.startswith("MemAvailable:"):
                return int(ln.split()[1]) / 1024 / 1024
    except OSError:
        pass
    return 1e9


def failure_text(r: subprocess.CompletedProcess[str], what: str) -> str:
    """Why an `abt` call failed, for the model: the exit status, and stderr's
    first 2,000 characters with repeated lines once. A process killed by a
    signal (the out-of-memory killer) writes nothing; say so."""
    if r.returncode < 0:
        head = (
            f"{what} was killed by signal {-r.returncode} before it finished: on this machine that is the out-of-memory killer "
            "(a futures run streams every minute bar of its roots from the `need`'s date). Narrow the window or the roots, "
            "put `need bars @1m from` at the window's start less the warm-up, and run again."
        )
    else:
        head = f"{what} failed (exit {r.returncode})."
    seen: set[str] = set()
    lines = []
    for ln in (r.stderr or r.stdout or "").splitlines():
        t = ln.strip()
        if t and t not in seen and not t.startswith("warning [W"):
            seen.add(t)
            lines.append(t)
    detail = "\n".join(lines)[:2000]
    return head + ("\n" + detail if detail else "")


# The metric rows the model reads of an outcome (the full JSON stays on disk).
_RETURNS = ("n", "total_return", "cagr", "volatility", "sharpe", "max_drawdown", "max_drawdown_duration")
_TRADING = ("decisions", "fills", "turnover", "dropped")


def brief_outcome(o: dict[str, Any]) -> dict[str, Any]:
    """An outcome's step, point and the metrics that matter, rounded."""

    def pick(d: Any, keys: tuple[str, ...] | None) -> dict[str, Any]:
        if not isinstance(d, dict):
            return {}
        items = d.items() if keys is None else ((k, d.get(k)) for k in keys)
        return {k: (round(v, 5) if isinstance(v, float) else v) for k, v in items if isinstance(v, (int, float)) and not isinstance(v, bool)}

    return {
        "step": o.get("step"),
        "point": o.get("point"),
        "run_id": o.get("run_id"),
        "returns": pick(o.get("returns"), _RETURNS),
        "trading": pick(o.get("trading"), _TRADING),
        "trades": pick(o.get("trades"), None),
    }


class LocalAbt:
    """`Gateway`'s interface (`call`, `calls`, `answers`, `close`) over the `abt` CLI."""

    def __init__(self, h: str, work: Path | None = None) -> None:
        self.bin = os.environ.get("ABT_BIN", "/srv/absolute/absolute-backtest/target/release/abt")
        lib = os.environ.get("ABT_LIB", "/srv/absolute/absolute-backtest/corpus-v2/catalog/*.dsl /srv/absolute/absolute-backtest/corpus-v2/lib/*.dsl")
        self.lib: list[str] = sorted(p for pat in lib.split() for p in glob.glob(pat))
        self.data = os.environ.get("ABT_DATA", "/srv/absolute/run/data.sock")
        # Norgate's daily continuous futures (`<ROOT>_N`, one bar a session from each root's inception) on their own service.
        self.daily = os.environ.get("ABT_DATA_DAILY", "/srv/absolute/run/norgate-futures.sock")
        self.store = os.environ.get("ABT_STORE", "/srv/absolute/store")
        self.daily_store = os.environ.get("ABT_STORE_DAILY", "/srv/absolute/store-norgate-futures")
        # Norgate's equities: every US list and ETF from 1990, and the daily index series, on their own service.
        # Off (the S&P 500 since 2014 in the main store) until the equity service's memory is bounded: ABT_EQUITY_FULL=1.
        self.equity_full = os.environ.get("ABT_EQUITY_FULL") == "1"
        self.equity_data = os.environ.get("ABT_DATA_EQUITY") or ("/srv/absolute/run/equities.sock" if self.equity_full else "/srv/absolute/run/data.sock")
        self.equity_store = os.environ.get("ABT_STORE_EQUITY") or ("/srv/absolute/store-full" if self.equity_full else "/srv/absolute/store")
        self.work = work or Path(os.environ.get("ABT_WORK", "/tmp/abt-work"))
        self.work.mkdir(parents=True, exist_ok=True)
        self.roots = [r for r in os.environ.get("ABT_ROOTS", "").split(",") if r]
        self.h_text = h
        self.h = _parse_time(h)
        self.calls: list[tuple[str, dict[str, Any]]] = []
        self.answers: list[Any] = []
        self.plans: dict[str, dict[str, Any]] = {}

    def close(self) -> None:
        pass

    # ------------------------------------------------------------------ plumbing

    def call(self, route: str, body: dict[str, Any]) -> Any:
        self.calls.append((route, body))
        handler = getattr(self, f"_r_{route}", None)
        try:
            if handler is None:
                raise GatewayError("no_route", f"no route `{route}` in the local mode")
            result = handler(body)
        except GatewayError as e:
            self.answers.append(e.for_model())
            self._log(route, body, e.for_model())
            raise
        self.answers.append(result)
        self._log(route, body, result)
        return result

    def _log(self, route: str, body: dict[str, Any], answer: Any) -> None:
        with (self.work / "calls.jsonl").open("a", encoding="utf-8") as f:
            f.write(json.dumps({"route": route, "body": body, "answer": answer}, default=str)[:200_000] + "\n")

    def _abt(self, args: list[str], timeout: float = 3600.0) -> subprocess.CompletedProcess[str]:
        try:
            return subprocess.run([self.bin, *args], capture_output=True, text=True, timeout=timeout, cwd=self.work)
        except subprocess.TimeoutExpired as e:
            raise GatewayError("timeout", f"abt {args[0]} {args[1] if len(args) > 1 else ''} took over {timeout:.0f} s; narrow the window or the universe") from e

    def _write(self, name: str, text: str) -> Path:
        p = self.work / name
        p.write_text(text, encoding="utf-8")
        return p

    def _query_store(self, text: str, max_rows: int = MAX_ROWS, store: str | None = None) -> dict[str, Any]:
        r = self._abt(["data", "query", "--store", store or self.store, text], timeout=600)
        if r.returncode != 0:
            raise GatewayError("query", (r.stderr or r.stdout).strip()[:2000])
        return parse_table(r.stdout, max_rows)

    # ------------------------------------------------------------------ the catalog

    def _r_catalog(self, body: dict[str, Any]) -> Any:
        """The roots the store holds and the class's relations: what a program
        can name. Not the catalog's whole text (tens of kilobytes resent on
        every later turn)."""
        cls = str(body.get("class") or "future")
        pred = str(body.get("predicate") or "").strip()
        value = pred.split("=", 1)[-1].strip().strip("\"'").upper() if pred else ""
        out: dict[str, Any] = {"class": cls, "relations": self._signatures(cls)}
        if cls == "future":
            roots = [r for r in self.roots if not value or r == value]
            out["roots"] = [self._root_entry(r) for r in roots] if value else [{"root": r, "abt_id": self._root_ids().get(r)} for r in roots]
            if value and not roots:
                out["note"] = f"{value} is not a staged root; staged: {','.join(self.roots)}"
            out["bars"] = "back-adjusted continuous series per root at @1m from 2010-06-07 (intraday ideas); daily views by `@session` on a `sessions` calendar"
            daily = [r for r in self.daily_roots() if not value or r in (value, f"{value}_N", f"{value}_U")]
            if daily:
                out["daily_roots"] = daily
            out["daily"] = (
                "DAILY ideas run on the same Databento roots: a `sessions` calendar and `decide on session`, the session's bars "
                "rolled up from minutes, from each root's first date (2010-06-07 for most) to H."
            )
        elif cls == "equity":
            out["universe"] = 'Norgate: every S&P 500 member since 2014-01-01, as of each day (member(A, T, "SPX")), daily bars' if not self.equity_full else (
                'Norgate: every US-listed stock and ETF, current and delisted, daily bars from 1990-01-02, point in time; lists by day '
                '(member(A, T, "<LIST>")): RALL, R3000, R2000, R1000, RMID, RMICRO, RTOP200, SPX, SP100, SP400, SP600, SP1500, NDX, NASDAQ, NYSE, DJIA, NBI, ETF; '
                'index series by `need series "<NAME>" @1d` (VIX_D, SPX, SPXTR, NDX, RUT …)'
            )
        return out

    def _signatures(self, cls: str) -> list[str]:
        """The class file's relation lines (`close(A: Future, @T, P: Price<USD>)`, …)."""
        sigs: list[str] = []
        for p in self.lib:
            if Path(p).name != f"{cls}.dsl":
                continue
            for ln in Path(p).read_text(encoding="utf-8").splitlines():
                t = ln.strip()
                if re.match(r"^[a-z_]+\(", t) or t.startswith(("lists ", "reasons ", "base ", "unit ")):
                    sigs.append(t.split("#", 1)[0].strip())
        return sigs

    def _root_ids(self) -> dict[str, int]:
        """The staged roots' data-layer ids: the `continuous` root ids, named by the `ticker` registry
        (equities share symbols: ES is a Norgate stock too, so only continuous roots are kept)."""
        if getattr(self, "_ids", None) is not None:
            return self._ids
        ids: dict[str, int] = {}
        try:
            roots = {int(r[0]) for r in self._query_store(f"continuous latest at {self.h_text}", 100_000)["rows"] if r and isinstance(r[0], int)}
            cache = Path(os.environ.get("ABT_TICKERS", "/srv/absolute/direct/ticker.json"))
            names = json.loads(cache.read_text(encoding="utf-8")) if cache.exists() else {}
            for i, sym in names.items():
                if int(i) in roots and sym in self.roots:
                    ids[sym] = int(i)
        except (GatewayError, ValueError, OSError):
            pass
        self._ids: dict[str, int] | None = ids
        return ids

    def _root_entry(self, root: str) -> dict[str, Any]:
        return {
            "root": root,
            "abt_id": self._root_ids().get(root),
            "query_key": f"{root}.v.0",
            "in_a_program": f"instruments {{ {root} from <date> }} in the universe block names the root by its symbol (as corpus-v2's r8l.dsl does)",
            "in_a_query": f'identity ["{root}.v.0"] (the continuous back-adjusted series)',
        }

    def _r_schema(self, body: dict[str, Any]) -> Any:
        cls = str(body.get("class") or "future")
        sigs = self._signatures(cls)
        return {"class": cls, "relations": sigs or [f"no catalog file for class {cls}"], "futures_roots_staged": self.roots if cls == "future" else None}

    def daily_roots(self) -> list[str]:
        """Norgate's daily continuous series in the daily store (`ES_N`, `ES_U`, …), from its ticker registry;
        none unless `ABT_NORGATE_FUTURES=1`: the owner's rule (Oct 20) is futures from Databento only."""
        if os.environ.get("ABT_NORGATE_FUTURES") != "1":
            return []
        if getattr(self, "_daily", None) is None:
            try:
                r = self._abt(["data", "query", "--store", self.daily_store, "ticker"], timeout=600)
                self._daily: list[str] | None = sorted(set(re.findall(r"\b([A-Z][A-Z0-9]*_[NU])\b", r.stdout)))
            except GatewayError:
                self._daily = []
        return self._daily or []

    def _r_resolve_symbols(self, body: dict[str, Any]) -> Any:
        out = []
        daily = set(self.daily_roots())
        for s in body.get("symbols") or []:
            sym = str(s).upper().lstrip("$").replace("/", "")
            root = sym[:-1] if sym.endswith("1!") else sym
            root = root[:-2] if root.endswith(("_N", "_U")) else root
            d = {"daily": f"{root}_N", "daily_unadjusted": f"{root}_U" if f"{root}_U" in daily else None, "daily_query_key": f"n:{root}"} if f"{root}_N" in daily else {}
            if root in self.roots:
                out.append({"symbol": s, "class": "future", "available": True, **self._root_entry(root), **d})
                continue
            if d:
                out.append({"symbol": s, "class": "future", "available": True, "root": root, "minute_store": False, **d,
                            "note": f"daily only: Norgate's {root}_N from the root's inception (no minute bars)"})
                continue
            try:
                t = self._query_store(f"ticker where symbol = {sym}", 5, store=self.equity_store)
                found = bool(t.get("rows"))
            except GatewayError:
                found = False
            out.append(
                {"symbol": s, "class": "equity" if found else None, "root": None, "available": found, "note": None if found else "not in the store (futures staged: " + ",".join(self.roots) + ")"}
            )
        return out

    # ------------------------------------------------------------------ data

    def _r_query(self, body: dict[str, Any]) -> Any:
        f = body.get("form") or {}
        rel = str(f.get("relation") or "")
        if not rel:
            raise GatewayError("malformed", "a query names its relation (class.relation)")
        kind = str(f.get("kind") or "scan")
        parts = [rel]
        if kind == "latest":
            parts.append("latest")
        if kind in ("point", "section", "latest") and (f.get("t") or f.get("t_max")):
            parts.append(f"at {f.get('t') or f.get('t_max')}")
        elif f.get("t0") or f.get("t1"):
            parts.append(f"over {f.get('t0') or '1900-01-01'}..{f.get('t1') or self.h_text}")
        ids = [self._key(str(i)) for i in (f.get("identity") or [])]
        pred = str(f.get("predicate") or "").strip()
        m = re.fullmatch(r"(?:root|symbol|A)\s*=\s*[\"']?([A-Za-z0-9.]+)[\"']?", pred)
        if m and rel.startswith("future."):
            ids.append(self._key(m.group(1)))
            pred = ""
        if ids:
            parts.append("for " + ",".join(ids))
        if pred:
            parts.append(f"where {pred}")
        r_max = self.h
        if body.get("r_max"):
            try:
                r_max = min(r_max, _parse_time(str(body["r_max"])))
            except ValueError:
                pass
        parts.append(f"r <= {_iso(r_max)}")
        daily = any(i.startswith(("n:", "u:")) for i in ids)
        equity = rel.startswith(("equity.", "series.")) or rel in ("ticker", "vendor_id") and not daily
        store = self.daily_store if daily else self.equity_store if equity else None
        table = self._query_store(" ".join(parts), int(body.get("limit") or 50), store=store)
        table["query"] = " ".join(parts)
        table["store"] = "daily futures (Norgate)" if daily else "equities and index series (Norgate)" if equity else "minute futures"
        return table

    def _key(self, ident: str) -> str:
        """A futures root's query key: the minute store's `NQ` -> `NQ.v.0`; the daily store's `ES_N` -> `n:ES`,
        `ES_U` -> `u:ES` (vendor keys); anything else as written."""
        up = ident.upper()
        if up in self.roots:
            return f"{up}.v.0"
        m = re.fullmatch(r"([A-Z][A-Z0-9]*)_([NU])", up)
        if m and up in set(self.daily_roots()):
            return f"{m.group(2).lower()}:{m.group(1)}"
        return ident

    # ------------------------------------------------------------------ programs

    def _program_files(self, programs: dict[str, str]) -> list[str]:
        paths = []
        for name, text in programs.items():
            fname = name if name.endswith(".dsl") else f"{name}.dsl"
            paths.append(str(self._write(fname, text)))
        return paths

    def _hash(self, name: str, files: list[str]) -> str:
        r = self._abt(["hash", "--strategy", name, *self.lib, *files], timeout=120)
        m = re.search(r"\b([0-9a-f]{64})\b", r.stdout)
        return m.group(1) if m else ""

    def _r_check(self, body: dict[str, Any]) -> Any:
        program = str(body.get("program") or "")
        name = (_STRATEGY.search(program) or [None, str(body.get("name") or "idea")])[1]
        files = self._program_files({f"{name}.dsl": program})
        r = self._abt(["check", *self.lib, *files], timeout=300)
        diags = []
        for ln in (r.stdout + "\n" + r.stderr).splitlines():
            m = _DIAG.match(ln.strip())
            if m:
                diags.append({"severity": m.group(1), "code": m.group(2), "message": m.group(3)})
        dof = next((ln.split(":", 1)[1].strip() for ln in r.stdout.splitlines() if "degrees of freedom" in ln), None)
        errors = [d for d in diags if d["severity"] == "error"]
        result: dict[str, Any] = {"diagnostics": diags, "degrees_of_freedom": dof, "summary": r.stdout.strip().splitlines()[-1] if r.stdout.strip() else r.stderr.strip()[-400:]}
        if not errors:
            result["program_hash"] = self._hash(name, files)
        return result

    def _r_contract(self, body: dict[str, Any]) -> Any:
        program = str(body.get("program") or "")
        name = (_STRATEGY.search(program) or [None, "idea"])[1]
        files = self._program_files({f"{name}.dsl": program})
        r = self._abt(["contract", "--strategy", name, *self.lib, *files], timeout=300)
        return {"text": (r.stdout or r.stderr)[:MAX_TEXT]}

    def _r_show(self, body: dict[str, Any]) -> Any:
        program = str(body.get("program") or "")
        name = (_STRATEGY.search(program) or [None, "idea"])[1]
        files = self._program_files({f"{name}.dsl": program})
        what = str(body.get("what") or "program")
        args = ["show", "plan", "--strategy", name] if what == "plan" else ["show", "--strategy", name]
        r = self._abt([*args, *self.lib, *files], timeout=300)
        return {"text": (r.stdout or r.stderr)[:MAX_TEXT]}

    # ------------------------------------------------------------------ plans

    def guard_plan(self, plan: str, programs: dict[str, str] | None = None) -> str:
        """The rules that survive the cut; the plan's text, unchanged, when it passes. The window's
        start is never limited: the owner's rule is as much history as the store holds (Oct 19)."""
        w = _WINDOW.search(plan)
        if not w:
            raise GatewayError("holdout", f"a plan writes `window <start> .. {self.h_text}` (the window ends at H)")
        end = w.group(4)
        try:
            end_dt = _parse_time(end)
        except ValueError as e:
            raise GatewayError("holdout", f"the window's end `{end}` is not a time; write `.. {self.h_text}`") from e
        if end_dt != self.h:
            raise GatewayError("holdout", f"the window ends at H: write `window {w.group(2)} .. {self.h_text}`; the plan says `.. {end}`")
        try:
            start_dt = _parse_time(w.group(2))
        except ValueError as e:
            raise GatewayError("window", f"the window's start `{w.group(2)}` is not a date") from e
        h = _HOLDOUT.search(plan)
        if not h or h.group(1) != "trailing:20%":
            raise GatewayError("holdout", "the development plan's hold-out is `holdout trailing:20%`")
        b = _BUDGET.search(plan)
        if not b or not re.search(r"\bmoney\s+0\b(?![.\d])", b.group(1)):
            raise GatewayError("budget", "the plan's budget is `budget { money 0; bytes 2G; requests 0 }`: no data is bought")
        return plan

    def _data_for(self, programs: Any) -> str:
        """The daily store's socket for a program on Norgate roots (`ES_N`), else the minute store's."""
        texts = [str(t) for t in (programs.values() if isinstance(programs, dict) else [programs])]
        if any(re.search(r"\bclass\s+equity\b", t) for t in texts):
            return self.equity_data
        norgate = os.environ.get("ABT_NORGATE_FUTURES") == "1" and any(_NORGATE_ROOT.search(t) for t in texts)
        return self.daily if norgate else self.data

    def _plan_files(self, body: dict[str, Any]) -> tuple[Path, list[str], str]:
        programs = {str(k): str(v) for k, v in (body.get("programs") or {}).items()}
        plan = self.guard_plan(str(body.get("plan") or ""), programs)
        files = self._program_files(programs)
        m = re.search(r"^\s*study\s+([A-Za-z_]\w*)", plan, re.M)
        study = m.group(1) if m else "idea"
        path = self._write(f"{study}.study", plan)
        return path, files, study

    def _r_plan_check(self, body: dict[str, Any]) -> Any:
        path, files, _ = self._plan_files(body)
        r = self._abt(["plan", "check", str(path), *files, *self.lib, "--data", self._data_for(body.get("programs") or {}), "--json"], timeout=900)
        if r.returncode != 0:
            raise GatewayError("plan", (r.stderr or r.stdout).strip()[-3000:])
        try:
            return json.loads(r.stdout)
        except ValueError:
            return {"text": r.stdout[:MAX_TEXT]}

    def _r_plan_run(self, body: dict[str, Any]) -> Any:
        path, files, study = self._plan_files(body)
        kernels = "1"  # one binding per slot: two full-history bindings (5+ GB each) overran the 10 GB slot (Oct 20, 06:00)
        r = self._abt(["plan", "run", str(path), *files, *self.lib, "--data", self._data_for(body.get("programs") or {}), "--kernels", kernels, "--json"], timeout=3000)
        if r.returncode != 0:
            raise GatewayError("plan", failure_text(r, "abt plan run"))
        run = json.loads(r.stdout)
        plan_hash = str(run.get("hash", ""))
        reveal = None
        steps = re.findall(r"^\s*reveal\s+(\w+)\s+of", str(body.get("plan") or ""), re.M)
        for step in steps[:1]:
            rv = self._abt(["plan", "reveal", str(path), step, *files, *self.lib, "--data", self._data_for(body.get("programs") or {}), "--kernels", "1", "--json"], timeout=3000)
            if rv.returncode == 0:
                try:
                    reveal = json.loads(rv.stdout)
                except ValueError:
                    reveal = {"text": rv.stdout[:MAX_TEXT]}
            else:
                reveal = {"error": failure_text(rv, "abt plan reveal")}
        tables = self._tables(plan_hash)
        runs = [
            {
                "step": o.get("step"),
                "run_id": o.get("run_id"),
                "status": o.get("status"),
                "point": o.get("point"),
                "decisions": int((o.get("trading") or {}).get("decisions") or (o.get("trading") or {}).get("trades") or o.get("decisions") or (o.get("returns") or {}).get("n") or 0),
                "returns": o.get("returns"),
                "trading": o.get("trading"),
            }
            for o in run.get("outcomes", [])
        ]
        report = {"plan_run": run, "reveal": reveal, "tables": tables}
        record = {"plan_hash": plan_hash, "study": study, "plan": str(body.get("plan") or ""), "programs": body.get("programs") or {}, "runs": runs, "report": report}
        self.plans[plan_hash] = record
        (self.work / f"plan-{plan_hash[:16]}.json").write_text(json.dumps(record, default=str), encoding="utf-8")
        (self.work / "last_plan.json").write_text(json.dumps(record, default=str), encoding="utf-8")
        self._nav(record)
        brief = {
            "plan_hash": plan_hash,
            "program_hash": run.get("program_hash"),
            "lineage": run.get("lineage"),
            "runs": [{k: v for k, v in x.items() if k in ("step", "run_id", "status", "point", "decisions")} for x in runs],
            "outcomes": [brief_outcome(o) for o in run.get("outcomes", [])][:12],
            "grids": run.get("grids"),
            "dsr": run.get("dsr"),
            "trial_kinds": run.get("trial_kinds"),
            "reveal": [brief_outcome(o) for o in (reveal or {}).get("outcomes", [])] if isinstance(reveal, dict) else reveal,
            "warnings": [w.get("message", "")[:300] for w in run.get("warnings", [])][:8],
        }
        return {"plan_hash": plan_hash, "runs": brief["runs"], "report": brief}

    def _tables(self, plan_hash: str) -> dict[str, Any]:
        out: dict[str, Any] = {}
        if not plan_hash:
            return out
        for t in ("summary", "contribution", "subperiods", "surface", "trials", "claims", "contract", "warnings"):
            try:
                out[t] = self._query_store(f"query.{plan_hash}.report.{t}", 2000)
            except GatewayError as e:
                out[t] = {"error": str(e)[:300]}
        return out

    def _nav(self, record: dict[str, Any]) -> None:
        """The NAV of the base run and of the reveal (the whole window), as CSV beside the plan."""
        report = record["report"]
        wanted = []
        rv = report.get("reveal")
        if isinstance(rv, dict):
            for o in rv.get("outcomes", []):
                if o.get("run_id"):
                    wanted.append(("reveal", o["run_id"]))
        for r in record["runs"]:
            if r.get("step") == "base" and r.get("run_id"):
                wanted.append(("base", r["run_id"]))
        for label, run_id in wanted:
            try:
                t = self._query_store(f"kernel.{run_id}.book", 100_000)
            except GatewayError:
                continue
            cols = t.get("columns", [])
            if "t" not in cols or "nav" not in cols:
                continue
            ti, ni = cols.index("t"), cols.index("nav")
            ii = cols.index("instrument") if "instrument" in cols else None
            lines = ["t,nav"]
            for row in t["rows"]:
                if ii is not None and row[ii] not in (None, ""):
                    continue
                lines.append(f"{row[ti]},{row[ni]}")
            (self.work / f"nav-{label}-{record['plan_hash'][:16]}.csv").write_text("\n".join(lines) + "\n", encoding="utf-8")

    def _r_plan_report(self, body: dict[str, Any]) -> Any:
        ph = str(body.get("plan_hash") or "")
        rec = self.plans.get(ph)
        if rec is None:
            raise GatewayError("no_plan", f"{ph} is not a plan this job ran")
        run = rec["report"]["plan_run"]
        tables = rec["report"]["tables"]
        return {
            "summary": [brief_outcome(o) for o in run.get("outcomes", [])],
            "grids": run.get("grids"),
            "dsr": run.get("dsr"),
            "trials": {"count": run.get("trials"), "kinds": run.get("trial_kinds")},
            "claims": run.get("claims"),
            "reveal": rec["report"].get("reveal") and [brief_outcome(o) for o in rec["report"]["reveal"].get("outcomes", [])],
            "contribution": {k: v for k, v in (tables.get("contribution") or {}).items() if k != "text"},
            "warnings": [w.get("message", "")[:300] for w in run.get("warnings", [])][:12],
        }

    def _r_query_run(self, body: dict[str, Any]) -> Any:
        """The kernel's query language on a run this job made (`? BODY …`, `why`, `why not`, `trace R at DATE`,
        `coverage over …`, `show …`, `ledger R`): `abt query` re-runs the run's program at its point over its
        window and answers. Minutes on futures; ask several questions in one call, separated by `;;`."""
        run_id = str(body.get("run_id") or "")
        text = str(body.get("text") or "").strip()
        if not text:
            raise GatewayError("malformed", "a query_run names its text")
        found = None
        for rec in self.plans.values():
            for r in rec.get("runs") or []:
                if r.get("run_id") == run_id:
                    found = (rec, r)
        if found is None:
            raise GatewayError("no_run", f"{run_id} is not a run of a plan this job ran")
        rec, run = found
        files = self._program_files(rec.get("programs") or {})
        program = next(iter((rec.get("programs") or {}).values()), "")
        name = (_STRATEGY.search(program) or [None, "idea"])[1]
        plan = str(rec.get("plan") or "")
        m = re.search(r"^\s*window\s+(\S+)\s*\.\.\s*(\S+)", plan, re.M)
        step = str(run.get("step") or "")
        # The step's own `over D1 .. D2`, up to the next step on the same line.
        over = re.search(rf"\brun\s+{re.escape(step)}\b(?:(?!\b(?:run|grid|reveal|report|compare|walk_forward)\b)[^\n])*?\bover\s+(\S+)\s*\.\.\s*(\S+)", plan) if step else None
        start, end = (over.group(1), over.group(2)) if over else ((m.group(1), m.group(2)) if m else ("", self.h_text))
        args = ["query", "--strategy", name, "--data", self._data_for(program)]
        if start:
            args += ["--start", start[:10]]
        args += ["--end", (end or self.h_text)[:10]]
        for k, v in run.get("point") or []:
            args += ["--param", f"{k}={v}"]
        for q in [t.strip() for t in text.split(";;") if t.strip()][:6]:
            args += ["--text", q]
        r = self._abt([*args, *self.lib, *files], timeout=1200)
        if r.returncode != 0:
            raise GatewayError("query_run", failure_text(r, "abt query"))
        lines = [ln for ln in r.stdout.splitlines() if not ln.startswith("warning [W")]
        return {"run_id": run_id, "window": f"{start} .. {end}", "point": run.get("point"), "text": "\n".join(lines)[:MAX_TEXT]}

    def _r_submit(self, body: dict[str, Any]) -> Any:
        sid = hashlib.blake2b(json.dumps(body, sort_keys=True, default=str).encode("utf-8"), digest_size=32).hexdigest()
        (self.work / "submission.json").write_text(json.dumps({"submission_id": sid, **body}, default=str), encoding="utf-8")
        return {"submission_id": sid, "forward_plan_hash": ""}

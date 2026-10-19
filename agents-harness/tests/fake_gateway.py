"""A fake gateway (contracts section 5): the routes with canned answers, the
token's caps enforced (403 for what it lacks), every request recorded. The
fake token is base64 JSON `{"caps": [...], "h": ..., "plan_runs": n,
"grid_points": n}`; a real Biscuit is opaque to the harness, which only
forwards it, so the fake decodes what the gateway would verify."""

from __future__ import annotations

import base64
import hashlib
import json
import re
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

ROUTE_CAP = {
    "catalog": "abt.catalog",
    "schema": "abt.catalog",
    "resolve_symbols": "abt.catalog",
    "query": "abt.query",
    "check": "abt.check",
    "contract": "abt.check",
    "show": "abt.check",
    "plan_check": "abt.plan",
    "plan_run": "abt.plan",
    "plan_report": "abt.report",
    "query_run": "abt.query_run",
    "submit": "abt.submit",
    "provenance": "provenance.write",
}


def fake_token(caps: list[str], h: str, plan_runs: int = 4, grid_points: int = 25, job: str = "job") -> str:
    return base64.b64encode(json.dumps({"caps": caps, "h": h, "plan_runs": plan_runs, "grid_points": grid_points, "job": job}).encode()).decode()


def _hash(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()


def _table(columns: list[str], rows: list[list[Any]]) -> dict[str, Any]:
    return {"columns": columns, "rows": rows, "truncated": False}


class FakeGateway:
    def __init__(self) -> None:
        self.requests: list[tuple[str, str, dict[str, Any]]] = []  # (route, token, body)
        self.lock = threading.Lock()
        self.busy_once: set[str] = set()  # routes that answer 429 on their first call
        self.plan_runs_used = 0
        self.plans: dict[str, dict[str, Any]] = {}
        self.programs: dict[str, str] = {}
        self.run_ids: set[str] = set()
        self.submitted: dict[str, Any] | None = None
        self.run_counter = 0
        gw = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a: Any) -> None:
                pass

            def do_POST(self) -> None:
                n = int(self.headers.get("Content-Length", "0"))
                body = json.loads(self.rfile.read(n) or b"{}")
                m = re.match(r"^(?:/gateway)?/v1/([a-z_]+)$", self.path)
                route = m.group(1) if m else ""
                auth = self.headers.get("Authorization", "")
                token = auth.removeprefix("Biscuit ").strip() if auth.startswith("Biscuit ") else ""
                with gw.lock:
                    gw.requests.append((route, token, body))
                    code, payload = gw.answer(route, token, body)
                data = json.dumps(payload).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self.httpd.server_port}/gateway"

    def start(self) -> FakeGateway:
        self.thread.start()
        return self

    def stop(self) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()

    def routes_called(self) -> list[str]:
        return [r for r, _, _ in self.requests]

    # ---------------------------------------------------------------- answers

    def answer(self, route: str, token: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
        try:
            facts = json.loads(base64.b64decode(token))
        except Exception:  # noqa: BLE001
            return 403, {"ok": False, "error": {"code": "token", "message": "the token does not verify"}}
        if route == "health":
            return 200, {"ok": True, "result": {"data": "ok", "kernels": "ok", "query": "ok"}}
        cap = ROUTE_CAP.get(route)
        if cap is None:
            return 403, {"ok": False, "error": {"code": "no_route", "message": f"no route {route!r} for this token"}}
        if cap not in facts.get("caps", []):
            return 403, {"ok": False, "error": {"code": "forbidden", "message": f"the token lacks cap({cap!r})"}}
        if route in self.busy_once:
            self.busy_once.discard(route)
            return 429, {"ok": False, "error": {"code": "busy", "message": "the kernel queue is full"}}
        return 200, {"ok": True, "result": getattr(self, f"r_{route}")(facts, body)}

    def r_catalog(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        rows = [["future", "ES", 1, "2000-01-03", "2026-10-16", "@1m"], ["future", "NQ", 2, "2000-01-03", "2026-10-16", "@1m"], ["future", "CL", 3, "2007-01-03", "2026-10-16", "@1m"]]
        if body.get("predicate"):
            rows = [r for r in rows if r[1] in body["predicate"]]
        return _table(["class", "root", "abt_id", "listed_from", "listed_to", "base"], rows)

    def r_schema(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        return {
            "text": 'class future {\n  base @1m\n  unit contracts\n  close(A: Future, @T, P: Price<USD>)\n  session(A: Future, @S, Open: Timestamp) complete\n  point_value(A: Future, @T, V: Scalar) complete\n  commission(A: Future, @T, C: Notional<USD>) complete\n  member(A: Future, @T, List: Label) complete\n  lists "HOUSE"\n}'
        }

    def r_resolve_symbols(self, facts: dict[str, Any], body: dict[str, Any]) -> list[dict[str, Any]]:
        known = {"ES": 1, "NQ": 2, "CL": 3, "QQQ": 2, "SPY": 1}
        return [
            {
                "symbol": s,
                "abt_id": known.get(s.upper()),
                "class": "future" if s.upper() in known else None,
                "root": {"QQQ": "NQ", "SPY": "ES"}.get(s.upper(), s.upper()),
                "listed_from": "2000-01-03",
                "listed_to": "2026-10-16",
            }
            for s in body.get("symbols", [])
        ]

    def r_query(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        return _table(["identity", "t", "r", "value"], [["ES", "2019-01-02T21:00:00Z", "2019-01-02T21:00:01Z", 2510.25]])

    def r_check(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        program = str(body.get("program", ""))
        if "BUG" in program:
            return {"diagnostics": [{"code": "M", "severity": "error", "message": "`V` is bound by no literal; `atr` needs it", "span": [12, 3]}]}
        h = _hash(program)
        self.programs[h] = program
        return {"diagnostics": [{"code": "W5", "severity": "warning", "message": "a structural constant", "span": [40, 2]}] if "W5" in program else [], "program_hash": h, "degrees_of_freedom": 4}

    def r_contract(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        return {"clauses": [{"name": "causality", "status": "held"}, {"name": "costs_active", "status": "held"}]}

    def r_show(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        return {"text": "decide <- enter <- atr <- true_range <- close", "tables": []}

    def r_plan_check(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        plan = str(body.get("plan", ""))
        steps = re.findall(r"\b(run|grid|reveal|compare|report)\s+(\w+)", plan)
        return {"hash": _hash(plan), "normal_form": plan.strip(), "trials_expected": len(steps), "steps": [f"{k} {n}" for k, n in steps]}

    def r_plan_run(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        plan = str(body.get("plan", ""))
        if self.plan_runs_used >= int(facts.get("plan_runs", 4)):
            raise _Refuse(409, "plan_runs", "the job's plan executions are spent")
        self.plan_runs_used += 1
        programs = body.get("programs", {})
        program = next(iter(programs.values()), "")
        plan_hash = _hash(plan)
        runs = []
        for name in re.findall(r"\brun\s+(\w+)", plan):
            self.run_counter += 1
            run_id = f"run_{self.run_counter:03d}"
            self.run_ids.add(run_id)
            runs.append({"step": name, "run_id": run_id, "status": "ok", "decisions": 0 if "NO_TRADES" in program else 37})
        report = self.report_for(plan)
        self.plans[plan_hash] = {"plan": plan, "program_hash": _hash(program), "report": report}
        return {"plan_hash": plan_hash, "runs": runs, "report": report}

    def report_for(self, plan: str) -> dict[str, Any]:
        return {
            "summary": _table(
                ["step", "window", "return", "sharpe", "mar", "max_drawdown", "trades", "profit_factor", "hit_rate", "decisions"],
                [["base", "insample", 0.08, 0.9, 0.5, 0.16, 260, 1.4, 0.44, 260], ["base", "oos", 0.05, 0.6, 0.3, 0.1, 61, 1.3, 0.42, 61]],
            ),
            "trials": _table(["kind", "count", "var_sharpe"], [["run", 6, 0.1], ["grid", 25, 0.2], ["reveal", 1, 0.0], ["claim", 2, 0.0]]),
            "claims": _table(
                ["name", "test", "phase", "verdict", "statistic", "p", "p_adjusted"],
                [["survives_costs", "bootstrap_sharpe@1", "after", "holds", 0.9, 0.01, 0.02], ["not_overfit", "pbo@1", "after", "holds", 0.3, None, None]],
            ),
            "contract": _table(["clause", "status"], [["causality", "held"], ["costs_active", "held"]]),
            "warnings": _table(["code", "message"], []),
            "biases": _table(["bias", "status"], [["lookahead", "clear"], ["survivorship", "clear"], ["multiplicity", "counted"]]),
            "surface": _table(["p1", "p2", "sharpe"], [[20, 2, 0.8], [50, 3, 0.9]]),
            "subperiods": _table(["year", "return", "share_of_profit"], [[2019, 0.1, 0.5], [2020, 0.06, 0.5]]),
            "contribution": _table(["market", "net_profit", "share", "profitable"], [["ES", 40000, 0.5, True], ["NQ", 40000, 0.5, True]]),
            "text": "The base run survives costs in-sample; the out-of-sample Sharpe is 0.6.",
        }

    def r_plan_report(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        p = self.plans.get(str(body.get("plan_hash", "")))
        if p is None:
            raise _Refuse(403, "forbidden", "not a plan of this job")
        return p["report"]

    def r_query_run(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        if str(body.get("run_id", "")) not in self.run_ids:
            raise _Refuse(403, "forbidden", "not a run of this job")
        text = str(body.get("text", ""))
        if text.startswith("ledger"):
            return _table(["A", "T", "Q", "P"], [["ES", "2019-02-06T14:31:00Z", 3.6058, 2731.25], ["ES", "2019-03-04T14:31:00Z", -3.6058, 2792.5]])
        return {"text": f"answer to {text!r}"}

    def r_submit(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        if self.submitted is not None:
            raise _Refuse(409, "submitted", "one submit per job")
        if body.get("plan_hash") not in self.plans:
            raise _Refuse(409, "lineage", "the plan was not run in this job")
        if self.plans[body["plan_hash"]]["program_hash"] != body.get("program_hash"):
            raise _Refuse(409, "lineage", "the program hash is not the plan's")
        self.submitted = body
        return {"submission_id": _hash(json.dumps(body, sort_keys=True)), "forward_plan_hash": _hash(str(body.get("forward_plan_text", "")))}

    def r_provenance(self, facts: dict[str, Any], body: dict[str, Any]) -> dict[str, Any]:
        return {"written": len(body.get("documents", [])) + len(body.get("hypotheses", []))}


class _Refuse(Exception):
    def __init__(self, status: int, code: str, message: str) -> None:
        super().__init__(message)
        self.status = status
        self.code = code
        self.message = message


def _patched_answer(self: FakeGateway, route: str, token: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
    try:
        return FakeGateway._answer_unpatched(self, route, token, body)
    except _Refuse as e:
        return e.status, {"ok": False, "error": {"code": e.code, "message": e.message}}


FakeGateway._answer_unpatched = FakeGateway.answer  # type: ignore[attr-defined]
FakeGateway.answer = _patched_answer  # type: ignore[method-assign]

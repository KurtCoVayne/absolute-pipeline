"""The tools (PL-6.4): exactly one per granted capability, each a call on the
gateway's JSON API (contracts section 5), a document read from `/job/in`, or a
note. A tool for an ungranted capability does not exist in the agent."""

from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass, field
from typing import Any

from agents import FunctionTool, Tool, function_tool
from pydantic import BaseModel, Field

from . import guards
from .budget import Meter
from .envelope import envelope
from .gateway import Gateway, GatewayError
from .job import Inputs
from .local import LocalAbt
from .skills import blake3_hex

# A tool answer is resent on every later turn: keep it small (the full
# answers stay on disk for the site). Tables show TABLE_ROWS rows, long texts
# TEXT_CHARS characters, lists LIST_ITEMS items; the whole answer at most
# MAX_TOOL_OUTPUT characters.
MAX_TOOL_OUTPUT = 16_000
TABLE_ROWS = 50
TEXT_CHARS = 6_000
LIST_ITEMS = 50


class ProtocolEnd(Exception):
    """The job ends here with a reason of guideline section 14."""

    def __init__(self, reason: str, detail: str) -> None:
        super().__init__(f"{reason}: {detail}")
        self.reason = reason
        self.detail = detail


@dataclass
class PlanRun:
    plan: str
    plan_hash: str
    program_name: str
    program: str
    program_hash: str
    holdout: str
    runs: list[dict[str, Any]]
    report: dict[str, Any] | None
    reported: bool = False


@dataclass
class QuantState:
    """What the quant has done so far: the sequence the harness checks (guideline section 9)."""

    checks_used: int = 0
    plan_attempts: int = 0
    # Runs that decided nothing and are not yet explained by a `why not`/`trace`/`coverage` query.
    zero_runs: list[str] = field(default_factory=list)
    resolved: set[str] = field(default_factory=set)
    clean: dict[str, str] = field(default_factory=dict)  # program hash -> text, as the checker accepted it
    names: dict[str, str] = field(default_factory=dict)  # program hash -> strategy name
    first_program_hash: str | None = None
    plan_runs_used: int = 0
    plans: dict[str, PlanRun] = field(default_factory=dict)
    run_ids: set[str] = field(default_factory=set)
    query_runs: int = 0
    base_decisions: int | None = None
    submission: dict[str, Any] | None = None
    submission_record: dict[str, Any] | None = None

    def clean_hash_of(self, program: str) -> str | None:
        for h, text in self.clean.items():
            if text == program:
                return h
        return None


@dataclass
class Context:
    inputs: Inputs
    meter: Meter
    gateway: Gateway | LocalAbt | None
    notes: list[str] = field(default_factory=list)
    quant: QuantState = field(default_factory=QuantState)
    skills_hash: str = ""
    prompt_hash: str = ""

    @property
    def h(self) -> str:
        return guards.h_text(self.inputs.job.h)


class QueryForm(BaseModel):
    """One of the query forms of P-3.2 (contracts section 5, `query`)."""

    kind: str = Field(description="point | latest | series | section | scan | exists | count | distinct")
    relation: str = Field(description="class.relation, e.g. future.close")
    identity: list[str] = Field(description="the instrument ids; empty for all")
    t: str = Field(description="ISO time for point; empty otherwise")
    t_max: str = Field(description="ISO time for latest; empty otherwise")
    t0: str = Field(description="ISO start for series/section/scan; empty otherwise")
    t1: str = Field(description="ISO end; empty otherwise")
    predicate: str = Field(description="a filter, e.g. root = ES; empty for none")
    columns: list[str] = Field(description="the columns wanted; empty for all")


def compact(value: Any) -> Any:
    """A tool answer as the model needs it: a table's first TABLE_ROWS rows
    with the total, a long text's head with its length, a long list's head
    with its count, repeated diagnostics once with their count."""
    if isinstance(value, dict):
        if isinstance(value.get("rows"), list) and isinstance(value.get("columns"), list):
            rows = value["rows"]
            out = {k: compact(v) for k, v in value.items() if k != "rows"}
            out["rows"] = rows[:TABLE_ROWS]
            if len(rows) > TABLE_ROWS:
                out["rows_total"] = len(rows)
                out["truncated"] = True
                out["hint"] = f"{len(rows) - TABLE_ROWS} more rows: narrow the window or the identity"
            return out
        return {k: compact(v) for k, v in value.items() if k not in ("normal_form",)}
    if isinstance(value, list):
        if value and all(isinstance(d, dict) and "message" in d for d in value):
            seen: dict[tuple[Any, str], dict[str, Any]] = {}
            for d in value:
                key = (d.get("code"), str(d.get("message", ""))[:160])
                if key in seen:
                    seen[key]["count"] = seen[key].get("count", 1) + 1
                else:
                    seen[key] = dict(d)
            value = list(seen.values())
        head = [compact(v) for v in value[:LIST_ITEMS]]
        if len(value) > LIST_ITEMS:
            head.append(f"… {len(value) - LIST_ITEMS} more")
        return head
    if isinstance(value, str) and len(value) > TEXT_CHARS:
        return value[:TEXT_CHARS] + f"… [{len(value) - TEXT_CHARS} more characters]"
    return value


def _dumps(value: Any) -> str:
    text = json.dumps(compact(value), ensure_ascii=False, separators=(",", ":"), default=str)
    if len(text) > MAX_TOOL_OUTPUT:
        return text[:MAX_TOOL_OUTPUT] + f'… [truncated: {len(text) - MAX_TOOL_OUTPUT} more characters; narrow the request]"}}'
    return text


def _refused(code: str, message: str) -> str:
    return _dumps({"ok": False, "error": {"code": code, "message": message, "refused_by": "harness"}})


def _gateway(ctx: Context, route: str, body: dict[str, Any]) -> Any:
    if ctx.gateway is None:
        raise GatewayError("no_gateway", "this job has no gateway (ABT_GATEWAY unset)")
    return ctx.gateway.call(route, body)


def _call(ctx: Context, route: str, body: dict[str, Any]) -> str:
    try:
        return _dumps({"ok": True, "result": _gateway(ctx, route, body)})
    except GatewayError as e:
        return _dumps(e.for_model())


def _program_file(name: str) -> str:
    return name if name.endswith(".dsl") else f"{name}.dsl"


# ------------------------------------------------------------------ per capability


def _docs_read(ctx: Context) -> list[FunctionTool]:
    def read_document(doc_id: str) -> str:
        """Read one input document (or one of its provenance closure) inside its envelope. Only the job's documents exist."""
        ctx.meter.add_tool_call()
        doc = ctx.inputs.docs.get(doc_id)
        if doc is None or doc_id not in ctx.inputs.closure:
            return _refused("no_document", f"{doc_id} is not one of this job's documents: {ctx.inputs.closure}")
        return envelope(doc)

    return [function_tool(read_document, failure_error_function=None)]


def _notes_write(ctx: Context) -> list[FunctionTool]:
    def note(text: str) -> str:
        """Append a note to the job's notes (notes.md): assumptions, the translation check, injection attempts, what was left out."""
        ctx.meter.add_tool_call()
        ctx.notes.append(text.strip())
        return _dumps({"ok": True, "notes": len(ctx.notes)})

    return [function_tool(note, failure_error_function=None)]


def _abt_catalog(ctx: Context) -> list[FunctionTool]:
    def catalog(class_name: str, predicate: str) -> str:
        """The registry as a table: `class_name` one of future, equity, series, option (or empty for all); `predicate` such as `root = ES` (or empty)."""
        ctx.meter.add_tool_call()
        body: dict[str, Any] = {}
        if class_name:
            body["class"] = class_name
        if predicate:
            body["predicate"] = predicate
        return _call(ctx, "catalog", body)

    def schema(class_name: str) -> str:
        """The class catalog as `abt data catalog` prints it: the primitives, the catalog relations and the vocabularies (lists, labels)."""
        ctx.meter.add_tool_call()
        return _call(ctx, "schema", {"class": class_name})

    def resolve_symbols(symbols: list[str]) -> str:
        """What the author's symbols are in the store: abt_id, class, root, listed_from, listed_to."""
        ctx.meter.add_tool_call()
        try:
            result = _gateway(ctx, "resolve_symbols", {"symbols": symbols})
        except GatewayError as e:
            return _dumps(e.for_model())
        for r in result if isinstance(result, list) else []:
            if isinstance(r, dict) and r.get("available"):
                ctx.quant.resolved.add(str(r.get("root") or r.get("symbol") or ""))
        return _dumps({"ok": True, "result": result})

    return [function_tool(f, failure_error_function=None) for f in (catalog, schema, resolve_symbols)]


def _abt_query(ctx: Context) -> list[FunctionTool]:
    def query(form: QueryForm, r_max: str) -> str:
        """A data query (P-3.2's forms), clamped to the job's horizon H by the gateway; at most 500 rows. `r_max` an ISO time or empty."""
        ctx.meter.add_tool_call()
        f: dict[str, Any] = {"kind": form.kind, "relation": form.relation}
        for k in ("identity", "columns"):
            if getattr(form, k):
                f[k] = getattr(form, k)
        for k in ("t", "t_max", "t0", "t1", "predicate"):
            if getattr(form, k):
                f[k] = getattr(form, k)
        body: dict[str, Any] = {"form": f}
        if r_max:
            body["r_max"] = r_max
        return _call(ctx, "query", body)

    return [function_tool(query, failure_error_function=None)]


def _abt_check(ctx: Context) -> list[FunctionTool]:
    q = ctx.quant
    checks_cap = ctx.inputs.job.budget.checks or 0

    def check(program: str, name: str) -> str:
        """Check a program: the diagnostics (code, severity, message, span), the program hash and the degrees of freedom when it is clean. Bounded by the job's `checks`."""
        ctx.meter.add_tool_call()
        q.checks_used += 1
        if checks_cap and q.checks_used > checks_cap:
            raise ProtocolEnd("could_not_check", f"{checks_cap} check iterations without a clean program; the last diagnostics are in the notes")
        try:
            guards.check_execution(program)
        except guards.Refused as e:
            return _refused(e.code, str(e))
        try:
            result = _gateway(ctx, "check", {"program": program, "name": name})
        except GatewayError as e:
            return _dumps(e.for_model())
        diagnostics = result.get("diagnostics", []) if isinstance(result, dict) else []
        errors = [d for d in diagnostics if str(d.get("severity", "")).lower() == "error"]
        program_hash = str(result.get("program_hash") or "") if isinstance(result, dict) else ""
        if not errors and program_hash:
            q.clean[program_hash] = program
            q.names[program_hash] = guards.program_name(program) or name
        elif errors:
            ctx.notes.append(f"check {q.checks_used}/{checks_cap or '∞'} of `{name}`: " + "; ".join(f"[{d.get('code')}] {d.get('message')}" for d in errors[:6]))
        return _dumps({"ok": True, "result": result, "checks_used": q.checks_used, "checks_left": max(0, checks_cap - q.checks_used) if checks_cap else None})

    def contract(program: str, name: str) -> str:
        """The kernel's contract clauses for a program (K-3.7)."""
        ctx.meter.add_tool_call()
        return _call(ctx, "contract", {"program": program, "name": name})

    def show(program: str, name: str, what: str) -> str:
        """`abt show` for a program: `what` is program | plan | calendars | execution."""
        ctx.meter.add_tool_call()
        return _call(ctx, "show", {"program": program, "name": name, "what": what})

    return [function_tool(f, failure_error_function=None) for f in (check, contract, show)]


def _validate_plan(ctx: Context, plan: str, program_name: str, program: str) -> tuple[str, guards.Holdout]:
    """The harness's guards on a plan before the gateway's: a checked program, a house
    execution block, the house hold-out, `after` claims, `revises` on a revision."""
    q = ctx.quant
    program_hash = q.clean_hash_of(program)
    if program_hash is None:
        raise guards.Refused("unchecked", "the program is not one the checker accepted clean in this job (the text differs, or `check` was not called); check it first")
    guards.check_execution(program)
    named = guards.plan_program(plan)
    if named is None:
        raise guards.Refused("program", "the plan names no `program <slug>.dsl`")
    if named != _program_file(program_name):
        raise guards.Refused("program", f"the plan's `program {named}` is not the program passed ({_program_file(program_name)})")
    holdout = guards.plan_holdout(plan, ctx.h)
    guards.require_after_claims(plan)
    if holdout.kind == "forward":
        guards.forbid_reveal(plan)
    if q.first_program_hash is not None and program_hash != q.first_program_hash and guards.revises(program) is None:
        raise guards.Refused("lineage", f"a second program carries `# revises: <program_hash>` naming the one it revises (the first was {q.first_program_hash})")
    return program_hash, holdout


def _abt_plan(ctx: Context) -> list[FunctionTool]:
    q = ctx.quant
    cap = ctx.inputs.job.budget.plan_runs or 0

    def plan_check(plan: str, program_name: str, program: str) -> str:
        """Validate a plan without running it: its hash, normal form, trials_expected and steps, after the gateway's guards. `program_name` is the slug the plan's `program <slug>.dsl` names; `program` its text (checked clean)."""
        ctx.meter.add_tool_call()
        q.plan_attempts += 1
        gate = zero_gate(ctx)
        if gate:
            return _refused("zero_decisions", gate)
        try:
            _validate_plan(ctx, plan, program_name, program)
        except guards.Refused as e:
            return _refused(e.code, str(e))
        return _call(ctx, "plan_check", {"plan": plan, "programs": {_program_file(program_name): program}})

    def plan_run(plan: str, program_name: str, program: str) -> str:
        """Run a plan (counted against plan_runs and grid_points): the plan hash, the runs with their decisions, and the report. `program_name` is the slug the plan's `program <slug>.dsl` names; `program` its text (checked clean)."""
        ctx.meter.add_tool_call()
        q.plan_attempts += 1
        gate = zero_gate(ctx)
        if gate:
            return _refused("zero_decisions", gate)
        try:
            program_hash, holdout = _validate_plan(ctx, plan, program_name, program)
        except guards.Refused as e:
            return _refused(e.code, str(e))
        if cap and q.plan_runs_used >= cap:
            return _refused("plan_runs", f"the job's {cap} plan executions are spent")
        q.plan_runs_used += 1
        try:
            result = _gateway(ctx, "plan_run", {"plan": plan, "programs": {_program_file(program_name): program}})
        except GatewayError as e:
            if e.code in {"plan_runs", "grid", "holdout", "lineage", "preset", "reveal", "budget"}:
                ctx.notes.append(f"plan_run refused by the gateway ({e.code}): {e.message}")
            return _dumps(e.for_model())
        if q.first_program_hash is None:
            q.first_program_hash = program_hash
        runs = list(result.get("runs", [])) if isinstance(result, dict) else []
        plan_hash = str(result.get("plan_hash", "")) if isinstance(result, dict) else ""
        for r in runs:
            if r.get("run_id"):
                q.run_ids.add(str(r["run_id"]))
            if r.get("step") == "base" and holdout.kind == "development":
                q.base_decisions = int(r.get("decisions") or 0)
        q.plans[plan_hash] = PlanRun(
            plan=plan,
            plan_hash=plan_hash,
            program_name=program_name,
            program=program,
            program_hash=program_hash,
            holdout=holdout.kind,
            runs=runs,
            report=result.get("report") if isinstance(result, dict) else None,
        )
        zero = [r for r in runs if int(r.get("decisions") or 0) == 0 and r.get("run_id")]
        answer: dict[str, Any] = {"ok": True}
        if zero:
            q.zero_runs = [str(r["run_id"]) for r in zero]
            steps = [r.get("step") for r in zero]
            ctx.notes.append(f"plan {plan_hash[:12]}: steps with decisions = 0: {steps} (a failure, never a result)")
            answer["attention"] = (
                f"DECISIONS = 0 in {steps}: the program traded nothing. Before any other plan or the end of the job, ask the kernel why: "
                f'query_run("{q.zero_runs[0]}", "coverage over <window>"), then "why not decide(T, _) at <bar>" or "trace decide at <bar>".'
            )
        answer.update({"result": result, "plan_runs_used": q.plan_runs_used, "plan_runs_left": max(0, cap - q.plan_runs_used) if cap else None})
        return _dumps(answer)

    return [function_tool(f, failure_error_function=None) for f in (plan_check, plan_run)]


def _abt_report(ctx: Context) -> list[FunctionTool]:
    q = ctx.quant

    def plan_report(plan_hash: str) -> str:
        """The report of a plan this job ran: summary, trials, claims, contract, warnings, biases, surface, subperiods, contribution, text."""
        ctx.meter.add_tool_call()
        if plan_hash not in q.plans:
            return _refused("no_plan", f"{plan_hash} is not a plan this job ran: {list(q.plans)}")
        try:
            result = _gateway(ctx, "plan_report", {"plan_hash": plan_hash})
        except GatewayError as e:
            return _dumps(e.for_model())
        q.plans[plan_hash].reported = True
        if isinstance(result, dict):
            q.plans[plan_hash].report = result
        return _dumps({"ok": True, "result": result})

    return [function_tool(plan_report, failure_error_function=None)]


def _abt_query_run(ctx: Context) -> list[FunctionTool]:
    q = ctx.quant

    def query_run(run_id: str, text: str) -> str:
        """Ask the kernel about a run this job made: `? …`, `why …`, `why not …`, `trace decide at …`, `coverage over …`, `show …`, `ledger R`."""
        ctx.meter.add_tool_call()
        if run_id not in q.run_ids:
            return _refused("no_run", f"{run_id} is not a run this job made: {sorted(q.run_ids)}")
        q.query_runs += 1
        if run_id in q.zero_runs and re.search(r"\b(why\s+not|trace|coverage)\b", text):
            q.zero_runs = []  # one explaining query on a silent run of the plan diagnoses the program
        return _call(ctx, "query_run", {"run_id": run_id, "text": text})

    return [function_tool(query_run, failure_error_function=None)]


def _submit_prerequisites(ctx: Context, plan_hash: str, forward_plan_text: str, rationale: str) -> PlanRun:
    q = ctx.quant
    if q.submission is not None:
        raise guards.Refused("submitted", "this job already submitted; a second submit is refused")
    pr = q.plans.get(plan_hash)
    if pr is None:
        raise guards.Refused("no_plan", f"{plan_hash} is not a plan this job ran")
    if pr.holdout != "development":
        raise guards.Refused("holdout", "the submitted plan is the development plan (holdout trailing, window ending at H)")
    if not pr.reported:
        raise guards.Refused("unread", "read the development plan's report first (plan_report)")
    if q.query_runs == 0:
        raise guards.Refused("translation_check", "no query_run was made: the translation check prints the trades, the indicator values and the fills through query_run before anything is submitted")
    if not guards.has_translation_check(ctx.notes):
        raise guards.Refused(
            "translation_check",
            "the notes carry no translation check: a note headed `## Translation check` with each trade's P&L recomputed to the cent (contracts × (exit − entry) × point_value − commission) beside the kernel's ledger",
        )
    if not guards.has_comparison_table(rationale):
        raise guards.Refused("comparison", "the rationale has no author comparison table (a Markdown table with an Author column and an Ours column and at least two rows)")
    fwd_program = guards.plan_program(forward_plan_text)
    if fwd_program != _program_file(pr.program_name):
        raise guards.Refused("program", f"the forward plan names `program {fwd_program}`; the submitted program is {_program_file(pr.program_name)}")
    holdout = guards.plan_holdout(forward_plan_text, ctx.h)
    if holdout.kind != "forward":
        raise guards.Refused("holdout", f"the forward plan's hold-out is `holdout after {ctx.h}`")
    guards.require_after_claims(forward_plan_text)
    guards.forbid_reveal(forward_plan_text)
    return pr


def _claims_of(plan: str) -> list[str]:
    block = guards._CLAIMS_BLOCK.search(plan)
    if not block:
        return []
    return [ln.strip() for ln in block.group(1).splitlines() if ln.strip() and not ln.strip().startswith("#")]


def _insample_of(pr: PlanRun) -> dict[str, Any]:
    report = pr.report or {}
    return {
        "verdicts": report.get("claims"),
        "metrics": report.get("summary"),
        "decisions": {str(r.get("step")): int(r.get("decisions") or 0) for r in pr.runs},
    }


def _abt_submit(ctx: Context) -> list[FunctionTool]:
    q = ctx.quant
    from .records import render_submission

    def submit(plan_hash: str, forward_plan_text: str, rationale: str) -> str:
        """Submit: the development plan's hash (run and reported in this job), the forward plan text (`holdout after H`, the same program, `after` claims) and the five-part rationale with the author comparison table. The harness fills the program, the plan, the claims and the in-sample results. One per job."""
        ctx.meter.add_tool_call()
        try:
            pr = _submit_prerequisites(ctx, plan_hash, forward_plan_text, rationale)
        except guards.Refused as e:
            return _refused(e.code, str(e))
        job = ctx.inputs.job
        hypothesis_id = str((ctx.inputs.hypothesis or {}).get("id", ""))
        claims = _claims_of(pr.plan)
        insample = _insample_of(pr)
        body = {
            "hypothesis_id": hypothesis_id,
            "program_hash": pr.program_hash,
            "plan_hash": pr.plan_hash,
            "program_text": pr.program,
            "plan_text": pr.plan,
            "forward_plan_text": forward_plan_text,
            "claims": claims,
            "rationale": rationale,
            "insample": insample,
        }
        try:
            result = _gateway(ctx, "submit", body)
        except GatewayError as e:
            return _dumps(e.for_model())
        q.submission = result if isinstance(result, dict) else {"submission_id": str(result)}
        trials = 0
        report = pr.report or {}
        trials_table = report.get("trials")
        if isinstance(trials_table, dict):
            try:
                cols = trials_table.get("columns", [])
                ci = cols.index("count") if "count" in cols else None
                if ci is not None:
                    trials = sum(int(row[ci]) for row in trials_table.get("rows", []))
            except (ValueError, TypeError, IndexError):
                trials = 0
        q.submission_record = render_submission(
            hypothesis_id=hypothesis_id,
            program_hash=pr.program_hash,
            plan_hash=pr.plan_hash,
            lineage=job.lineage or pr.program_hash,
            h=job.h,
            claims=claims,
            insample=insample,
            trials_used=trials,
            rationale=rationale,
            program_text=pr.program,
            plan_text=pr.plan,
            forward_plan_text=forward_plan_text,
            forward_plan_hash=str(q.submission.get("forward_plan_hash", "")),
            agent=ctx.inputs.definition.name,
            model=job.model,
            cost_usd=0.0,
            job_id=job.id,
            campaign=job.campaign,
            skills_hash=ctx.skills_hash,
        )
        return _dumps({"ok": True, "result": q.submission})

    return [function_tool(submit, failure_error_function=None)]


BUILDERS = {
    "docs.read": _docs_read,
    "notes.write": _notes_write,
    "abt.catalog": _abt_catalog,
    "abt.query": _abt_query,
    "abt.check": _abt_check,
    "abt.plan": _abt_plan,
    "abt.report": _abt_report,
    "abt.query_run": _abt_query_run,
    "abt.submit": _abt_submit,
}


MIN_CHECKS_BEFORE_GIVING_UP = 3


def zero_gate(ctx: Context) -> str | None:
    """While a run that decided nothing is unexplained, no other plan runs and
    the job does not end: ask the kernel why first (abt-idioms §6)."""
    q = ctx.quant
    if not q.zero_runs:
        return None
    rid = q.zero_runs[0]
    return (
        f"Refused: run {rid} decided nothing (decisions = 0) and was not asked why. A program that trades nothing is a translation "
        f'bug until shown otherwise. First: query_run("{rid}", "coverage over <the run\'s window>") names the first empty relation on '
        f'the path to decide; then query_run("{rid}", "why not decide(T, _) at <a bar you expected a trade>") or '
        f'query_run("{rid}", "trace decide at <that bar>"). Fix the rule it names, check again, then run the plan.'
    )


def _roots_in_hypothesis(ctx: Context) -> set[str]:
    """The staged roots (ABT_ROOTS) the hypothesis names, as words of its text."""
    roots = {r for r in os.environ.get("ABT_ROOTS", "").split(",") if r}
    text = json.dumps(ctx.inputs.hypothesis or {}, ensure_ascii=False).upper()
    return {r for r in roots if re.search(rf"(?<![A-Z0-9]){re.escape(r)}(?![A-Z0-9])", text)}


def end_refusal(ctx: Context, outcome: str) -> str | None:
    """Why the quant may not end with `outcome` yet, as a message it can act on;
    None when it may. `could_not_check` needs three checks; `not_testable`
    needs one check when the idea's roots are in the store. A job that ran or
    tried a plan has tried: its ending stands."""
    q = ctx.quant
    if outcome != "submitted" and zero_gate(ctx):
        return zero_gate(ctx)
    if outcome == "could_not_check" and q.checks_used < MIN_CHECKS_BEFORE_GIVING_UP and not q.plans and q.plan_attempts == 0:
        return (
            f"Refused: `could_not_check` needs at least {MIN_CHECKS_BEFORE_GIVING_UP} `check` calls and this job made {q.checks_used}. "
            "Write the program and check it; read every diagnostic and fix it. The language expresses intraday session windows "
            "(`abt-idioms` §2: decide at `session + 30min`, cumulative `close_at[M]` over `session(X, [0min, M))`), swings and "
            "Fibonacci levels (§3), calendar counts (§4) and weekly averages (§5); a vague rule takes a labelled assumption (`write-strategy`, step 1)."
        )
    if outcome == "not_testable" and q.checks_used == 0 and (q.resolved or _roots_in_hypothesis(ctx)):
        roots = ", ".join(sorted(q.resolved or _roots_in_hypothesis(ctx)))
        return (
            f"Refused: the idea's roots are in the store ({roots}) and no program was checked. `not_testable` is only for data the "
            "store cannot hold (positioning, implied volatility, breadth, fundamentals); a missing rule or a house-rule detail is a "
            "labelled assumption. Write the program from `abt-idioms` (§1 daily, §2 intraday session windows, §3 swings and "
            "Fibonacci, §4 calendar counts) and `check` it."
        )
    return None


def build_tools(ctx: Context) -> list[Tool]:
    """One tool set per granted capability, in the job's `caps` order; a cap
    without a builder here (`docs.search`, after the MVP) yields nothing."""
    tools: list[Tool] = []
    for cap in ctx.inputs.job.caps:
        builder = BUILDERS.get(cap)
        if builder is not None:
            tools.extend(builder(ctx))
    return tools


def tool_names(tools: list[Tool]) -> list[str]:
    return [t.name for t in tools]


def program_hash_of(program: str) -> str:
    """A local stand-in for H(P) when the gateway gave none (never the lineage's hash)."""
    return blake3_hex(program.encode("utf-8"))

---
name: abt-plans-and-claims
version: 2
---
# Plans and claims

A program is run only under a **plan** (`study`, Q-2.1), and every run is a counted trial of the program's lineage (Prop. Q-2.2). The plan says what runs, on what window, what is held out, what is claimed, and the gateway admits only plans in the house's shape (PL-6.4, PL-6.6). You pass plans to `plan_check(plan, program_name, program)` (validated, not run) and `plan_run(plan, program_name, program)` (run, counted), with `program_name` the slug the plan's `program <slug>.dsl` line names and `program` that program's text as the checker accepted it.

## 1. The syntax

```
study NAME {
  program    <slug>.dsl                        # the program; its normal-form hash fixes the lineage
  params     { k = v; … }                      # the base point (overrides defaults within their ranges)
  grid       { k = v1, v2, …; k2 = … }         # axes; the Cartesian product; ≤ grid_points (25) cells
  holdout    trailing:20% | after <H>          # the two house hold-outs (section 2); nothing else is admitted
  window     <first bar> .. <H>                # the report window of every run; ends at H for a development plan
  budget     { money 0; bytes 2G; requests 0 } # money 0 is forced (A-D22); bytes bound what resolving the demand may read
  objective  sharpe                            # what a grid orders by: sharpe | cagr | calmar | mar
  claims     { NAME : TEST on INPUT [with { … }] expect OUTCOME [at LEVEL] before|after [warn|block]   … }
  steps      { STEP … }
  report     { metrics [NAMES]; tables [NAMES]; text }
}
```

Steps, in a DAG by name:

| Step | Form | Counted |
| --- | --- | --- |
| `run` | `run NAME [at { k = v; … }] [over DATE .. DATE] [observe R, …]` | one trial |
| `grid` | `grid NAME` (the `grid` block) | one per cell |
| `reveal` | `reveal NAME of RUN` | one, as a reveal: lifts the embargo once for that run's point |
| `compare` | `compare NAME: RUN, RUN, … by [METRICS]` | no |
| `ledger` | `ledger NAME: R of RUN into TABLE` | no |
| `report` | `report NAME` | no |

Dates are `YYYY-MM-DD`; `H` is written as the gateway gives it to you (`YYYY-MM-DDTHH:MM:SSZ`), verbatim. A point `at { only = "ES" }` restricts the universe through a parameter the program declares (`only : Label = "all"`, read in `universe`).

## 2. The two hold-outs (house rules §10, PL-6.6)

1. **The development plan**: `holdout trailing:20%` with `window <first bar> .. <H>`. The embargo is the last 20 % of the span before `H`; every `run` and the `grid` are truncated at its first barrier (the kernel never sees the embargoed bars, Prop. Q-2.1); `reveal oos of base` runs the base point over the whole window and reports the embargoed bars only. One reveal, with the base parameters, never a point chosen by looking at it. `trailing:Ny` (years) and `trailing:Nmo` are admitted too; 20 % of the span is the house default.
2. **The forward plan**: `holdout after <H>`: the embargo starts at the last barrier received at or before `H`. The plan is pre-registered by `submit`; its reveals over `(H, now]` are the scheduler's, never yours. There is no `reveal` step in a forward plan.

The gateway refuses (`409`): a plan with any other hold-out (`holdout none`, `blocks:…`), a development window that does not end at `H`, a `reveal` in a forward plan, `budget` without `money 0`, a program whose hash is not in the job's lineage (the first program opens it; every later program carries `# revises: <program_hash>`), a grid over `grid_points`, a plan execution over `plan_runs`, and an `execution` block that is not a house preset or a named single-field variant. The harness refuses, before the gateway, a plan without `after` claims.

## 3. Claims (Q-5.1)

```
NAME : TEST on INPUT [over WINDOW] [with { param = value, … }] expect OUTCOME [at LEVEL] [before | after] [warn | block]
INPUT   = returns of RUN | returns of best(GRID) | feature R of RUN | events of RUN | feed RELATION[(A in (X, Y))] | grid NAME | ledger NAME
OUTCOME = reject | accept | statistic OP VALUE | effect OP VALUE | interval excludes VALUE
```

- `before` claims run on the in-sample window before any run (a feed or a feature from a warm-up run with no decisions); `after` claims run on outcomes. Every claim is re-tested on every reveal over the embargoed bars; the record is the pair (in-sample verdict, out-of-sample verdict).
- Verdicts: `holds`, `fails`, `undecidable(reason)` (an empty input, a precondition failed, a window inside the embargo). `warn` (default) records a failure; `block` stops the steps that depend on it.
- Within a plan, p-values are Benjamini–Hochberg adjusted over the plan's claims at 0.05; raw and adjusted are both recorded.

Tests you will use (Q-4.4): `bootstrap_sharpe` (stationary bootstrap interval of the Sharpe; the house's survives-costs test), `dsr` (deflated Sharpe over the lineage's trial count: `statistic > 0.95` means the Sharpe survives the number of trials), `psr`, `min_trl`, `pbo` (probability of backtest overfitting by CSCV over a grid: `< 0.5`), `nw_mean` (Newey–West t of the mean), `wfe`, `ljung_box`, `variance_ratio`, `arch_lm`, `adf`, `kpss`, `half_life`, `engle_granger`, `johansen`, `ic`, `fama_macbeth`, `coactivation`, `effective_rules`.

**The defaults** (house rules §11), always present unless the hypothesis states others:

```
claims {
  survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after
  not_overfit    : pbo on grid sens expect statistic < 0.5 after
}
```

Add `stated_sharpe : dsr on returns of base expect statistic > 0.95 after` when the author states a Sharpe; for a pair add the `before` claims `legs_cointegrate : engle_granger on feed close(A in (X, Y)) expect reject at 0.05 before`, `spread_stationary : adf on feature spread of warmup expect reject at 0.05 before`, `reverts : half_life on feature spread of warmup expect statistic < 60 before`. The forward plan carries `survives_costs` alone.

## 4. DSR, PBO and the trial count

- Every `run`, every grid cell, every reveal, every claim is a **trial** of the lineage; the gateway counts them all, including the translation check and the sanity run. The deflated Sharpe (`dsr`) deflates by the lineage's full count and the variance of the trials' Sharpes: a Sharpe of 1.2 after 30 trials may deflate below 0.95. You cannot lower the count; you can only avoid spending it on variants the author did not state.
- **PBO** over the grid asks whether the in-sample best cell is also good out of sample across combinatorial splits; above 0.5 the parameter choice is noise. A plateau gives a low PBO; a spike a high one.
- The report prints `trials` by kind and `var_trials`; the rationale states both.

## 5. The budgets are in plan executions and grid points, not trials

Your job admits `plan_runs` plan executions (4) and a grid of at most `grid_points` cells (25). The four are: the translation check's short plan, the development plan, one more (a retry after a failed sanity comparison **or** one execution-variant plan on a `# revises:` program), and the forward plan's `plan_check` (which the gateway validates when you `submit`; it does not execute). Spend them in that order; a fifth `plan_run` is `out_of_budget(plan_runs)` and the job ends.

## 6. The templates (house rules §15)

Every plan's `window` ends at `H` verbatim and starts no earlier than three years before it; `holdout trailing:20%`; `budget { money 0 … }`. The development plan is `write-strategy` step 4. The translation check is the same program over the last months before `H`:

```
study check_<slug> {
  program    <slug>.dsl
  params     { <the author's point> }
  holdout    trailing:20%
  window     <H − 4 months> .. <H>
  budget     { money 0; bytes 2G; requests 0 }
  objective  sharpe
  claims     { trades : nw_mean on returns of check expect statistic > -100 after }
  steps      { run check observe atr, size   report main }
  report     { metrics [return, trades]; tables [trades]; text }
}
```

Variants a parameter expresses (`side`, a stop) are `run … at { side = 1 }` steps inside the development plan; a variant of the `execution` block is a second program `# revises:` the first.

The forward plan (checked at `submit`; the same program hash, the submitted point):

```
study forward_<slug> {
  program    <slug>.dsl
  params     { <the submitted point> }
  holdout    after <H>
  budget     { money 0; bytes 2G; requests 0 }
  claims     { survives_costs : bootstrap_sharpe on returns of base expect interval excludes 0 at 0.05 after }
  steps      { run base   report main }
}
```

## 7. Hashes and identity

`plan_check` returns the plan's hash `H(Π)` over its normal form (template substituted, names canonicalised, steps in topological order, `report` removed) and `trials_expected`. `check` returns the program's hash `H(P)`. `submit` names both and the gateway verifies that the plan was run in this job with that program; nothing else is accepted.

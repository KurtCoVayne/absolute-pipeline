//! The per-job counters (PL-6.4, PL-6.6, PL-7.1): plan executions and
//! grid points against the token's budgets, the plans the job ran (their
//! hashes, what `submit` and `plan_report` are checked against), the runs
//! it created (what `query_run` may ask about), its one submission, the
//! bytes it read and its requests per minute. The only state the gateway
//! keeps beyond the queue; nothing of it is an artefact (A-D20).

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use crate::plan::Refusal;
use crate::token::Grant;

/// The default bytes a job may read through `query` (the guideline's
/// `bytes 2G` of a plan's budget, applied to the agent's own reads).
pub const DEFAULT_BYTES_PER_JOB: u64 = 2 << 30;
/// The default requests per minute per job.
pub const DEFAULT_REQUESTS_PER_MINUTE: u32 = 600;

/// What re-creates a plan's run on a kernel of the gateway's pool with the
/// same id (PL-D105): the program, the point, the window, the truncation,
/// the mask, the observations and π.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSpec {
    pub program: String,
    pub strategy: String,
    pub plan_hash: String,
    pub canonical: String,
    pub point: Vec<(String, String)>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub horizon: Option<i64>,
    pub mask: Vec<(i64, i64)>,
    pub observe: Vec<String>,
    pub fold: serde_json::Value,
}

#[derive(Clone, Debug, Default)]
pub struct JobState {
    pub plan_runs: u32,
    pub grid_points: u64,
    /// The plans the job ran: plan hash to program hash.
    pub plans: BTreeMap<String, String>,
    /// The runs the job created, with what re-creates each.
    pub runs: BTreeMap<String, Option<RunSpec>>,
    /// The runs held on the gateway's kernels for the job, oldest first.
    pub held: Vec<String>,
    pub submission: Option<String>,
    pub bytes: u64,
    minute: i64,
    in_minute: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Over {
    #[error("job {0} read {1} bytes, its budget")]
    Bytes(String, u64),
    #[error("job {0} made {1} requests this minute, its budget")]
    Rate(String, u32),
}

pub struct Jobs {
    jobs: Mutex<HashMap<String, JobState>>,
    bytes_per_job: u64,
    requests_per_minute: u32,
}

impl Jobs {
    pub fn new(bytes_per_job: u64, requests_per_minute: u32) -> Jobs {
        Jobs {
            jobs: Mutex::new(HashMap::new()),
            bytes_per_job,
            requests_per_minute,
        }
    }

    fn with<T>(&self, job: &str, f: impl FnOnce(&mut JobState) -> T) -> T {
        let mut m = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
        f(m.entry(job.to_string()).or_default())
    }

    pub fn state(&self, job: &str) -> JobState {
        self.with(job, |s| s.clone())
    }

    pub fn count(&self) -> usize {
        self.jobs.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// One more request of the job this minute (`now` in nanoseconds).
    pub fn tick(&self, job: &str, now: i64) -> Result<(), Over> {
        let limit = self.requests_per_minute;
        self.with(job, |s| {
            let minute = now.div_euclid(60 * pipeline_wire::NS_PER_S);
            if s.minute != minute {
                s.minute = minute;
                s.in_minute = 0;
            }
            s.in_minute += 1;
            if s.in_minute > limit {
                return Err(Over::Rate(job.to_string(), s.in_minute - 1));
            }
            Ok(())
        })
    }

    /// Bytes the job read, metered against its budget.
    pub fn meter(&self, job: &str, bytes: u64) -> Result<(), Over> {
        let cap = self.bytes_per_job;
        self.with(job, |s| {
            s.bytes += bytes;
            if s.bytes > cap {
                return Err(Over::Bytes(job.to_string(), s.bytes));
            }
            Ok(())
        })
    }

    /// Reserves one plan execution and `points` grid points of the job's
    /// budgets (PL-6.4): refused with `plan_runs` or `grid` when exhausted,
    /// before anything runs.
    pub fn reserve_plan(&self, grant: &Grant, points: u64) -> Result<(), Refusal> {
        self.with(&grant.job, |s| {
            if s.plan_runs >= grant.plan_runs {
                return Err(Refusal {
                    code: "plan_runs",
                    message: format!("job {} ran {} plans, its budget (out_of_budget(plan_runs))", grant.job, s.plan_runs),
                });
            }
            if s.grid_points + points > u64::from(grant.grid_points) {
                return Err(Refusal {
                    code: "grid",
                    message: format!(
                        "job {} would reach {} grid points with this plan; its budget is {}",
                        grant.job,
                        s.grid_points + points,
                        grant.grid_points
                    ),
                });
            }
            s.plan_runs += 1;
            s.grid_points += points;
            Ok(())
        })
    }

    /// Gives a reservation back: the plan did not run (a layer refused it
    /// before any run).
    pub fn release_plan(&self, job: &str, points: u64) {
        self.with(job, |s| {
            s.plan_runs = s.plan_runs.saturating_sub(1);
            s.grid_points = s.grid_points.saturating_sub(points);
        });
    }

    pub fn grid_points_left(&self, grant: &Grant) -> u64 {
        self.with(&grant.job, |s| u64::from(grant.grid_points).saturating_sub(s.grid_points))
    }

    /// Records what a plan execution produced.
    pub fn record_plan(&self, job: &str, plan_hash: &str, program_hash: &str, runs: impl IntoIterator<Item = (String, Option<RunSpec>)>) {
        self.with(job, |s| {
            s.plans.insert(plan_hash.to_string(), program_hash.to_string());
            for (id, spec) in runs {
                let e = s.runs.entry(id).or_insert(None);
                if spec.is_some() {
                    *e = spec;
                }
            }
        });
    }

    pub fn owns_run(&self, job: &str, run_id: &str) -> bool {
        self.with(job, |s| s.runs.contains_key(run_id))
    }

    pub fn run_spec(&self, job: &str, run_id: &str) -> Option<RunSpec> {
        self.with(job, |s| s.runs.get(run_id).cloned().flatten())
    }

    /// Notes a run held for the job; the run to release when the job holds
    /// more than `cap`.
    pub fn hold(&self, job: &str, run_id: &str, cap: usize) -> Option<String> {
        self.with(job, |s| {
            s.held.retain(|r| r != run_id);
            s.held.push(run_id.to_string());
            if s.held.len() > cap {
                Some(s.held.remove(0))
            } else {
                None
            }
        })
    }

    pub fn is_held(&self, job: &str, run_id: &str) -> bool {
        self.with(job, |s| s.held.iter().any(|r| r == run_id))
    }

    /// The program hash of a plan the job ran.
    pub fn program_of(&self, job: &str, plan_hash: &str) -> Option<String> {
        self.with(job, |s| s.plans.get(plan_hash).cloned())
    }

    /// Marks the job submitted; `Err` with the earlier submission's id when
    /// it already was (one per job).
    pub fn submit(&self, job: &str, submission_id: &str) -> Result<(), String> {
        self.with(job, |s| match &s.submission {
            Some(earlier) => Err(earlier.clone()),
            None => {
                s.submission = Some(submission_id.to_string());
                Ok(())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> Grant {
        Grant {
            job: "j".into(),
            agent: "quant".into(),
            caps: Default::default(),
            h: 0,
            lineage: String::new(),
            plan_runs: 2,
            grid_points: 5,
            expires: 1,
        }
    }

    #[test]
    fn budgets_are_counted_before_anything_runs() {
        let jobs = Jobs::new(100, 2);
        let g = grant();
        assert!(jobs.reserve_plan(&g, 3).is_ok());
        assert_eq!(jobs.reserve_plan(&g, 3).unwrap_err().code, "grid");
        assert_eq!(jobs.grid_points_left(&g), 2);
        assert!(jobs.reserve_plan(&g, 2).is_ok());
        assert_eq!(jobs.reserve_plan(&g, 0).unwrap_err().code, "plan_runs");
        jobs.release_plan("j", 2);
        assert!(jobs.reserve_plan(&g, 1).is_ok());
        jobs.record_plan("j", "p1", "h1", vec![("r1".to_string(), None)]);
        assert!(jobs.run_spec("j", "r1").is_none());
        assert_eq!(jobs.hold("j", "r1", 1), None);
        assert_eq!(jobs.hold("j", "r2", 1), Some("r1".into()));
        assert!(jobs.is_held("j", "r2") && !jobs.is_held("j", "r1"));
        assert!(jobs.owns_run("j", "r1"));
        assert!(!jobs.owns_run("j", "r2"));
        assert_eq!(jobs.program_of("j", "p1").as_deref(), Some("h1"));
        assert!(jobs.submit("j", "s1").is_ok());
        assert_eq!(jobs.submit("j", "s2"), Err("s1".into()));
        assert!(jobs.meter("j", 60).is_ok());
        assert_eq!(jobs.meter("j", 60), Err(Over::Bytes("j".into(), 120)));
        assert!(jobs.tick("j", 0).is_ok());
        assert!(jobs.tick("j", 0).is_ok());
        assert_eq!(jobs.tick("j", 0), Err(Over::Rate("j".into(), 2)));
        assert!(jobs.tick("j", 61 * pipeline_wire::NS_PER_S).is_ok());
    }
}

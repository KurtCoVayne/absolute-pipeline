//! The queue in front of the kernel pool (PL-7.1, PL-10): one permit per
//! kernel, one plan execution in flight per job, a per-job fair share
//! among the jobs waiting (round robin over jobs, first come first served
//! within one), `busy` past a configured depth (60 by default, GP-7's
//! bound). A lone job takes as many permits as the pool has free, so its
//! grid spreads over the kernels; under contention every job takes one.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

/// The queue's default depth past which it answers `busy`.
pub const DEFAULT_MAX_DEPTH: usize = 60;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    #[error("the kernel queue is {0} deep; try later")]
    Busy(usize),
    #[error("job {0} has a plan execution in flight; one at a time")]
    InFlight(String),
}

struct Waiter {
    ticket: u64,
    want: usize,
}

#[derive(Default)]
struct State {
    free: usize,
    /// The jobs with waiters, in the order they are served.
    order: VecDeque<String>,
    waiters: HashMap<String, VecDeque<Waiter>>,
    /// What each served ticket was granted.
    granted: HashMap<u64, usize>,
    in_flight: BTreeSet<String>,
    depth: usize,
}

pub struct Queue {
    permits: usize,
    max_depth: usize,
    state: Mutex<State>,
    moved: Condvar,
    next_ticket: AtomicU64,
    busy_total: AtomicU64,
    served_total: AtomicU64,
}

/// Permits held for one plan execution; released when dropped.
pub struct Permit<'a> {
    queue: &'a Queue,
    job: String,
    pub n: usize,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut st = self.queue.state.lock().unwrap_or_else(|p| p.into_inner());
        st.free += self.n;
        st.in_flight.remove(&self.job);
        self.queue.dispatch(&mut st);
        self.queue.moved.notify_all();
    }
}

impl Queue {
    pub fn new(permits: usize, max_depth: usize) -> Queue {
        Queue {
            permits: permits.max(1),
            max_depth,
            state: Mutex::new(State {
                free: permits.max(1),
                ..Default::default()
            }),
            moved: Condvar::new(),
            next_ticket: AtomicU64::new(1),
            busy_total: AtomicU64::new(0),
            served_total: AtomicU64::new(0),
        }
    }

    pub fn permits(&self) -> usize {
        self.permits
    }

    /// Requests waiting.
    pub fn depth(&self) -> usize {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).depth
    }

    /// Permits held.
    pub fn in_flight(&self) -> usize {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.permits - st.free
    }

    pub fn busy_total(&self) -> u64 {
        self.busy_total.load(Ordering::Relaxed)
    }

    pub fn served_total(&self) -> u64 {
        self.served_total.load(Ordering::Relaxed)
    }

    /// Hands free permits to waiters: the next job in the round robin, its
    /// first waiter; one permit each while other jobs wait, up to `want`
    /// for a job waiting alone.
    fn dispatch(&self, st: &mut State) {
        while st.free > 0 {
            let Some(job) = st.order.pop_front() else { break };
            let Some(q) = st.waiters.get_mut(&job) else { continue };
            let Some(w) = q.pop_front() else {
                st.waiters.remove(&job);
                continue;
            };
            let others_wait = !st.order.is_empty();
            let n = if others_wait { 1 } else { w.want.clamp(1, st.free) };
            st.free -= n;
            st.depth = st.depth.saturating_sub(1);
            st.granted.insert(w.ticket, n);
            st.in_flight.insert(job.clone());
            if q.is_empty() {
                st.waiters.remove(&job);
            } else {
                st.order.push_back(job);
            }
        }
    }

    /// Permits for one plan execution of `job`: at least one, up to `want`
    /// when the pool is otherwise idle; waits its turn. `Busy` when the
    /// queue is at its depth, `InFlight` when the job already runs a plan.
    pub fn acquire(&self, job: &str, want: usize) -> Result<Permit<'_>, QueueError> {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if st.in_flight.contains(job) || st.waiters.contains_key(job) {
                return Err(QueueError::InFlight(job.to_string()));
            }
            if st.depth >= self.max_depth {
                self.busy_total.fetch_add(1, Ordering::Relaxed);
                return Err(QueueError::Busy(st.depth));
            }
            st.depth += 1;
            st.waiters.entry(job.to_string()).or_default().push_back(Waiter { ticket, want: want.max(1) });
            if !st.order.iter().any(|j| j == job) {
                st.order.push_back(job.to_string());
            }
            self.dispatch(&mut st);
            loop {
                if let Some(n) = st.granted.remove(&ticket) {
                    self.served_total.fetch_add(1, Ordering::Relaxed);
                    self.moved.notify_all();
                    return Ok(Permit { queue: self, job: job.to_string(), n });
                }
                st = self.moved.wait(st).unwrap_or_else(|p| p.into_inner());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    #[test]
    fn a_lone_job_takes_the_pool_and_contending_jobs_share_it() {
        let q = Queue::new(4, 60);
        let p = q.acquire("a", 4).unwrap();
        assert_eq!(p.n, 4);
        assert_eq!(q.in_flight(), 4);
        assert!(matches!(q.acquire("a", 1), Err(QueueError::InFlight(j)) if j == "a"));
        drop(p);
        assert_eq!(q.in_flight(), 0);
        let p1 = q.acquire("a", 1).unwrap();
        let p2 = q.acquire("b", 4).unwrap();
        assert_eq!((p1.n, p2.n), (1, 3));
        drop(p1);
        drop(p2);
    }

    #[test]
    fn waiters_are_served_round_robin_over_jobs_and_busy_past_the_depth() {
        let q = Arc::new(Queue::new(1, 2));
        let held = q.acquire("holder", 1).unwrap();
        let served = Arc::new(Mutex::new(Vec::new()));
        let started = Arc::new(AtomicUsize::new(0));
        let mut hs = Vec::new();
        for job in ["a", "b"] {
            let (q, served, started) = (q.clone(), served.clone(), started.clone());
            hs.push(std::thread::spawn(move || {
                started.fetch_add(1, Ordering::SeqCst);
                let p = q.acquire(job, 4).unwrap();
                served.lock().unwrap().push((job, p.n));
            }));
        }
        while q.depth() < 2 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(q.acquire("c", 1), Err(QueueError::Busy(2))));
        assert_eq!(q.busy_total(), 1);
        drop(held);
        for h in hs {
            h.join().unwrap();
        }
        let s = served.lock().unwrap();
        assert_eq!(s.len(), 2);
        assert!(s.iter().all(|(_, n)| *n == 1 || *n == 4));
        assert_eq!(q.depth(), 0);
        assert_eq!(q.in_flight(), 0);
    }
}

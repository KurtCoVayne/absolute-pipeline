//! The topics of the log (PL-4.3): name, key, partitions, retention and
//! who produces and consumes them. The broker is created from this table
//! (`ingest topics create`), so the deployment and the code agree.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
    Days(u32),
    Unbounded,
    Compacted,
}

#[derive(Clone, Copy, Debug)]
pub struct Topic {
    pub name: &'static str,
    pub key: &'static str,
    pub partitions: i32,
    pub retention: Retention,
}

pub const RAW_X: &str = "raw.x";
pub const DOCS: &str = "docs";
pub const JOBS_TRIAGE: &str = "jobs.triage";
pub const JOBS_ANALYZE: &str = "jobs.analyze";
pub const JOBS_STRATEGIZE: &str = "jobs.strategize";
pub const TRIAGE: &str = "triage";
pub const HYPOTHESES: &str = "hypotheses";
pub const SUBMISSIONS: &str = "submissions";
pub const OUTCOMES: &str = "outcomes";
pub const CONTROL: &str = "control";

pub fn dlq(stage: &str) -> String {
    format!("dlq.{}", stage)
}

pub const TOPICS: &[Topic] = &[
    Topic {
        name: RAW_X,
        key: "account_id",
        partitions: 6,
        retention: Retention::Days(30),
    },
    Topic {
        name: DOCS,
        key: "author.id",
        partitions: 6,
        retention: Retention::Unbounded,
    },
    Topic {
        name: JOBS_TRIAGE,
        key: "input id",
        partitions: 6,
        retention: Retention::Days(7),
    },
    Topic {
        name: JOBS_ANALYZE,
        key: "input id",
        partitions: 6,
        retention: Retention::Days(7),
    },
    Topic {
        name: JOBS_STRATEGIZE,
        key: "input id",
        partitions: 6,
        retention: Retention::Days(7),
    },
    Topic {
        name: TRIAGE,
        key: "doc_id",
        partitions: 3,
        retention: Retention::Unbounded,
    },
    Topic {
        name: HYPOTHESES,
        key: "id",
        partitions: 3,
        retention: Retention::Unbounded,
    },
    Topic {
        name: SUBMISSIONS,
        key: "id",
        partitions: 3,
        retention: Retention::Unbounded,
    },
    Topic {
        name: OUTCOMES,
        key: "submission_id",
        partitions: 3,
        retention: Retention::Unbounded,
    },
    Topic {
        name: "dlq.triage",
        key: "job id",
        partitions: 1,
        retention: Retention::Days(30),
    },
    Topic {
        name: "dlq.analyze",
        key: "job id",
        partitions: 1,
        retention: Retention::Days(30),
    },
    Topic {
        name: "dlq.strategize",
        key: "job id",
        partitions: 1,
        retention: Retention::Days(30),
    },
    Topic {
        name: CONTROL,
        key: "subject",
        partitions: 1,
        retention: Retention::Compacted,
    },
];

/// Messages stay under 1 MiB (P-2.2 applied to the log, PL-4.3); larger
/// bodies go to the object store and the message carries the reference.
pub const MAX_MESSAGE_BYTES: usize = 1 << 20;
/// Raw bytes inline up to this size (PL-4.1).
pub const INLINE_BYTES: usize = 256 * 1024;

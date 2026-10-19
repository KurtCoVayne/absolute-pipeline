//! `abt-gateway` (docs/design/pipeline.md, PL-7; docs/design/contracts.md,
//! sections 5 and 6): the only entry to the three backtester services for
//! the pipeline. Two faces: towards the sandboxes a JSON tool API on a
//! loopback port, one route per capability of PL-6.4, every request
//! carrying the job's Biscuit token (`api`); towards the layers `abt-wire`
//! frames over the sockets of `abt-data`, the kernel pool and `abt-query`,
//! or over in-process channels in the tests (`layers`). Between the two,
//! policy and nothing else (PL-7.1's decision): the clamps on the horizon
//! `H` (`horizon`), the plan guards (`plan`), the per-job counters (`jobs`),
//! the queue in front of the kernel pool (`queue`), the `pipeline.*`
//! provenance and the audit (`provenance`), and the forward scheduler
//! (`forward`). No evaluation, no statistics, no storage of its own.

// An error carries what the caller acts on (the code, the message, the
// detail), as the backtester's `RunError` does; the size is the point.
#![allow(clippy::result_large_err)]

pub mod horizon;
pub mod jobs;
pub mod layers;
pub mod plan;
pub mod provenance;
pub mod query;
pub mod queue;
pub mod table;
pub mod token;

pub use layers::{LayerError, Layers};
pub use token::{Grant, Verifier};

/// The house presets of the guideline (section 15), as the library source
/// the gateway adds to every program it sends to the layers when the
/// library it was started with defines neither (the amendment of PL-8.1
/// puts them in `corpus-v2/lib/execution.dsl`; until it lands they travel
/// with the gateway, so a program naming them checks against the pinned
/// tree).
pub const HOUSE_PRESETS: &str = include_str!("house_presets.dsl");

/// The job the forward scheduler and the owner's commands act as.
pub const GATEWAY_JOB: &str = "gateway";

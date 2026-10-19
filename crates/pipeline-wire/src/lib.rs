//! The pipeline's records and nothing else (PL-4.1, PL-4.4, PL-P8): the
//! FlatBuffers tables compiled from `schema/pipeline.fbs`, their ids by
//! BLAKE3 over the RFC 8785 canonical JSON of the logical fields (the same
//! `Canonical` as `abt-wire`, never over the wire bytes), the time arithmetic
//! of the horizon (Definition PL-4.1: a post's fixity time from its id), the
//! topics of the log (PL-4.3), and a JSON rendering for people
//! (`pipeline wire decode`). No I/O, no broker client, no policy.

pub mod codec;
pub mod ids;
pub mod json;
pub mod time;
pub mod topics;

#[allow(clippy::all, clippy::pedantic, unused_imports, dead_code, non_camel_case_types, missing_docs)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/pipeline_generated.rs"));
}

pub use abt_wire::hash::Hash;
pub use codec::{decode, encode, CodecError};
pub use generated::pipeline::wire::{
    AccountState, Attachment, Author, Budget, BudgetRow, Campaign, Cost, Direction, Document, Enrichment, Entities, Envelope, Evidence, Failure, Flags, Follow, Hypothesis, Instrument, Job, JobState,
    Link, LinkState, Outcome, OutcomeStatus, Parent, ParentKind, RawItem, RawKind, Record, Segment, SegmentKind, Source, Stage, Submission, Tombstone, TombstoneSubject, Triage, Verdict,
};
pub use ids::{id_of, is_id, Id, Identified};
pub use time::{iso, parse_iso, snowflake_created, Time, NS_PER_MS, NS_PER_S};

/// The schema's version, carried in every topic's message headers.
pub const SCHEMA: &str = "pipeline.1";

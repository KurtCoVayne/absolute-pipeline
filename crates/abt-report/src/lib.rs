//! `abt-report` (PL-7.4, docs/design/contracts.md section 8): the static
//! report site. `inputs` gathers the log's records and the data layer's
//! tables into one `Inputs` (from a broker and a data socket, or from the
//! JSON fixtures); the pages follow.

pub mod data;
pub mod fixtures;
pub mod inputs;
pub mod log;
pub mod records;
pub mod sample;
pub mod stats;
pub mod table;

pub use inputs::{Inputs, Offset};
pub use table::Table;

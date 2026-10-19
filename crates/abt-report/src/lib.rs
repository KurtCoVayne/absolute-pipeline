//! `abt-report` (PL-7.4, docs/design/contracts.md section 8): the static
//! report site. `inputs` gathers the log's records and the data layer's
//! tables into one `Inputs` (from a broker and a data socket, or from the
//! JSON fixtures); `site` derives every page's view from them and `render`
//! writes the pages. Nothing here talks to a model or runs a backtest: the
//! site shows what the records and the report tables say, with the
//! guideline's red flags (§12) and the cohort's Benjamini–Hochberg
//! adjustment (PL-6.6 item 7) computed from them.

pub mod data;
pub mod fixtures;
pub mod flags;
pub mod inputs;
pub mod log;
pub mod records;
pub mod render;
pub mod sample;
pub mod site;
pub mod stats;
pub mod table;

pub use inputs::{Inputs, Offset};
pub use site::{build, Built};
pub use table::Table;

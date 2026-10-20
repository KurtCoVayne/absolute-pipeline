//! `seq(H)` (PL-7.1, Proposition PL-4.2): the last sequence number of a
//! relation's rows received by `H`. The pinned data layer exposes no such
//! number (`stats` has rows and spans, `stream_index` the barriers of a
//! stream, not the store's sequence), so the gateway finds it by a `scan`
//! of the relation with `with_seq` under the receipt filter `r ≤ H` and
//! keeps it per (relation, H): a job asks about few relations at one `H`.

use std::collections::HashMap;
use std::sync::Mutex;

use abt_wire::generated::abt::wire::{QueryForm, ReceiptFilter, Scan};
use arrow::array::{Array, AsArray};

use crate::layers::{LayerError, Layers};

#[derive(Default)]
pub struct Horizon {
    cache: Mutex<HashMap<(String, i64), Option<u64>>>,
}

impl Horizon {
    pub fn new() -> Horizon {
        Horizon::default()
    }

    /// The greatest `seq` among the rows of `relation` received by `h`;
    /// `None` when there is none (every row is after `h`, or the relation
    /// has no rows), in which case the receipt filter alone clamps.
    pub fn seq_of(&self, layers: &Layers, relation: &str, h: i64) -> Result<Option<u64>, LayerError> {
        let key = (relation.to_string(), h);
        if let Some(s) = self.cache.lock().expect("the seq cache").get(&key) {
            return Ok(*s);
        }
        let mut req = crate::query::request(QueryForm::Scan(Box::new(Scan {
            relation: relation.to_string(),
            t0: i64::MIN,
            t1: i64::MAX,
            predicate: None,
            columns: None,
        })));
        req.receipt_filter = Some(Box::new(ReceiptFilter { r_max: Some(h) }));
        req.with_seq = true;
        let (_, batches) = layers.query(req)?;
        let mut best: Option<u64> = None;
        for b in &batches {
            if let Some(col) = b.column_by_name("seq") {
                let s = col.as_primitive::<arrow::datatypes::UInt64Type>();
                for i in 0..s.len() {
                    if !s.is_null(i) {
                        best = Some(best.map_or(s.value(i), |m| m.max(s.value(i))));
                    }
                }
            }
        }
        self.cache.lock().expect("the seq cache").insert(key, best);
        Ok(best)
    }

    /// Forgets what was found: after a write the gateway made itself.
    pub fn forget(&self, relation: &str) {
        self.cache.lock().expect("the seq cache").retain(|(r, _), _| r != relation);
    }
}

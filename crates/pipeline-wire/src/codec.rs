//! One record on the wire: a size-prefixed `Envelope`, verified once on
//! receipt (P-2.2, PL-4.4).

use planus::{ReadAsRoot, WriteAsOffset};

use crate::generated::pipeline::wire::{Envelope, EnvelopeRef, Record};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("the message is not a pipeline envelope: {0}")]
    Malformed(String),
}

/// The bytes of one record.
pub fn encode(record: &Record) -> Vec<u8> {
    let mut b = planus::Builder::new();
    let env = Envelope { record: record.clone() };
    let off = env.prepare(&mut b);
    b.finish(off, None).to_vec()
}

/// The record in `bytes`, verified.
pub fn decode(bytes: &[u8]) -> Result<Record, CodecError> {
    let env = EnvelopeRef::read_as_root(bytes).map_err(|e| CodecError::Malformed(e.to_string()))?;
    let owned: Envelope = env.try_into().map_err(|e: planus::Error| CodecError::Malformed(e.to_string()))?;
    Ok(owned.record)
}

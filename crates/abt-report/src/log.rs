//! The log's side of the inputs (contracts.md section 8): the six topics
//! read from offset 0 to their high watermarks through `rdkafka`, every
//! envelope decoded, each message's topic and offset counted. A consumer
//! without durable state: the group is fresh per build and commits nothing,
//! so a rebuild is a re-read of the whole log, which is what makes the site
//! a function of the log.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::{ClientConfig, Message, Offset as KOffset, TopicPartitionList};

use crate::inputs::{Inputs, Offset};

/// The topics the site reads, in reading order (`control` first, so a
/// tombstone precedes the documents it empties in the output; the order
/// does not matter for correctness, `apply_tombstones` runs after all).
pub const TOPICS: &[&str] = &[
    pipeline_wire::topics::CONTROL,
    pipeline_wire::topics::DOCS,
    pipeline_wire::topics::TRIAGE,
    pipeline_wire::topics::HYPOTHESES,
    pipeline_wire::topics::SUBMISSIONS,
    pipeline_wire::topics::OUTCOMES,
];

/// Reads every topic in `topics` from the beginning into `inputs`;
/// `timeout` bounds each poll.
pub fn read(brokers: &str, topics: &[&str], inputs: &mut Inputs, timeout: Duration) -> Result<()> {
    let group = format!("abt-report-{}", pipeline_wire::time::now());
    let consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", &group)
        .set("enable.auto.commit", "false")
        .set("enable.partition.eof", "true")
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .create()
        .context("the broker client")?;
    for topic in topics {
        read_topic(&consumer, topic, inputs, timeout)?;
    }
    inputs.apply_tombstones();
    Ok(())
}

fn read_topic(consumer: &BaseConsumer, topic: &str, inputs: &mut Inputs, timeout: Duration) -> Result<()> {
    let meta = consumer.fetch_metadata(Some(topic), timeout).with_context(|| format!("{}: metadata", topic))?;
    let t = meta.topics().iter().find(|t| t.name() == topic).ok_or_else(|| anyhow!("{}: no such topic", topic))?;
    let mut high: BTreeMap<i32, i64> = BTreeMap::new();
    let mut tpl = TopicPartitionList::new();
    for p in t.partitions() {
        let (_, hi) = consumer.fetch_watermarks(topic, p.id(), timeout).with_context(|| format!("{}/{}: watermarks", topic, p.id()))?;
        high.insert(p.id(), hi);
        if hi > 0 {
            tpl.add_partition_offset(topic, p.id(), KOffset::Beginning)?;
        }
    }
    let mut next: BTreeMap<i32, i64> = high.keys().map(|p| (*p, 0)).collect();
    let mut count = 0u64;
    if !tpl.elements().is_empty() {
        consumer.assign(&tpl).with_context(|| format!("{}: assign", topic))?;
        let mut done: BTreeMap<i32, bool> = high.iter().map(|(p, hi)| (*p, *hi == 0)).collect();
        let mut idle = 0;
        while done.values().any(|d| !d) {
            match consumer.poll(timeout) {
                Some(Ok(m)) => {
                    idle = 0;
                    if let Some(payload) = m.payload() {
                        match pipeline_wire::decode(payload) {
                            Ok(r) => inputs.absorb(r),
                            Err(e) => tracing::warn!(topic, partition = m.partition(), offset = m.offset(), "undecodable message: {}", e),
                        }
                    }
                    count += 1;
                    next.insert(m.partition(), m.offset() + 1);
                    if m.offset() + 1 >= high[&m.partition()] {
                        done.insert(m.partition(), true);
                    }
                }
                Some(Err(rdkafka::error::KafkaError::PartitionEOF(p))) => {
                    done.insert(p, true);
                }
                Some(Err(e)) => return Err(anyhow!("{}: {}", topic, e)),
                None => {
                    idle += 1;
                    if idle >= 3 {
                        return Err(anyhow!("{}: the broker went quiet before the high watermark", topic));
                    }
                }
            }
        }
        consumer.unassign().ok();
    }
    inputs.offsets.push(Offset {
        input: topic.to_string(),
        detail: next.iter().map(|(p, o)| format!("{}:{}", p, o)).collect::<Vec<_>>().join(" "),
        records: count,
    });
    Ok(())
}

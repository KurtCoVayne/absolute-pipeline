//! The log's side against a broker (contracts.md section 1): skipped with a
//! reason when `PIPELINE_BROKERS` is unset; otherwise a few records are
//! produced to a fresh topic and read back from offset 0.

use std::time::Duration;

use pipeline_wire::{encode, Record};
use rdkafka::producer::{BaseProducer, BaseRecord, Producer};
use rdkafka::ClientConfig;

#[test]
fn the_topics_read_back_from_offset_zero() {
    let Ok(brokers) = std::env::var("PIPELINE_BROKERS") else {
        eprintln!("skipped: PIPELINE_BROKERS is unset (deploy/redpanda-local.sh starts one)");
        return;
    };
    let sample = abt_report::sample::generate();
    let topic = format!("abt-report-test-{}", pipeline_wire::time::now());
    let producer: BaseProducer = ClientConfig::new().set("bootstrap.servers", &brokers).set("enable.idempotence", "true").create().unwrap();
    let records: Vec<&Record> = sample.topics["hypotheses"].iter().chain(sample.topics["submissions"].iter()).collect();
    for r in &records {
        let bytes = encode(r);
        let key = pipeline_wire::json::render(r)["id"].as_str().unwrap().to_string();
        producer.send(BaseRecord::to(&topic).key(&key).payload(&bytes)).unwrap();
    }
    producer.flush(Duration::from_secs(10)).unwrap();
    let mut inputs = abt_report::Inputs::default();
    abt_report::log::read(&brokers, &[&topic], &mut inputs, Duration::from_secs(10)).unwrap();
    assert_eq!(inputs.hypotheses.len(), 12);
    assert_eq!(inputs.submissions.len(), 8);
    let o = inputs.offsets.iter().find(|o| o.input == topic).unwrap();
    assert_eq!(o.records, records.len() as u64);
}

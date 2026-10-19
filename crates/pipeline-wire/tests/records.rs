//! The records round-trip through the wire; the id is over the logical
//! fields and not over the bytes (PL-4.1, PL-4.4).

use pipeline_wire::{decode, encode, ids::Identified, json, snowflake_created, Author, Document, Job, Record, Source, Stage};
use proptest::prelude::*;

fn doc(text: &str, r: i64) -> Document {
    let post = 2101026622522429546u64;
    Document {
        id: String::new(),
        version: 1,
        raw_ids: Some(vec!["00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff".into()]),
        source: Source::X,
        author: Box::new(Author {
            id: "123".into(),
            handle: "peterlbrandt".into(),
        }),
        p: snowflake_created(post),
        fixed_at: snowflake_created(post),
        r,
        r_n: r,
        parents: None,
        text: text.into(),
        segments: None,
        entities: None,
        links: None,
        attachments: None,
        lang: Some("en".into()),
        flags: None,
        enrichment: None,
        source_deleted: false,
    }
}

#[test]
fn a_document_round_trips_and_its_id_ignores_the_wire_layout() {
    let mut d = doc("Gold breaks out of a 13-year cup and handle. $GC_F", 1_760_000_000_000_000_000);
    d.id = d.id();
    let bytes = encode(&Record::Document(Box::new(d.clone())));
    let back = decode(&bytes).unwrap();
    let Record::Document(b) = back else { panic!("not a document") };
    assert_eq!(*b, d);
    assert!(b.id_holds(&b.id));
    // A later version of the same document (new text, new r_n) keeps its id.
    let mut v2 = d.clone();
    v2.version = 2;
    v2.r_n = d.r + 1;
    v2.text = "the same post with its link fetched".into();
    assert_eq!(v2.id(), d.id);
    // A different receipt is a different document.
    let other = doc("Gold breaks out of a 13-year cup and handle. $GC_F", d.r + 1);
    assert_ne!(other.id(), d.id);
    let rendered = json::render(&Record::Document(Box::new(d.clone())));
    assert_eq!(rendered["record"], "Document");
    assert_eq!(rendered["id"].as_str().unwrap().len(), 64);
    assert_eq!(rendered["fixed_at"], "2026-09-18T19:12:36.653000000Z");
}

#[test]
fn a_job_id_is_over_stage_inputs_definition_skills_and_model() {
    let j = |attempt: u32, model: &str| Job {
        id: String::new(),
        stage: Stage::Triage,
        inputs: Some(vec!["ab".repeat(32)]),
        agent: "triage".into(),
        definition_hash: "cd".repeat(32),
        skills_hash: "ef".repeat(32),
        model: model.into(),
        caps: None,
        budget: None,
        attempt,
        state: pipeline_wire::JobState::Queued,
        sandbox: None,
        started: 0,
        finished: 0,
        cost: None,
        outputs: None,
        failure: None,
        h: 0,
        lineage: None,
        campaign: None,
    };
    assert_eq!(j(1, "gpt-6-luna").id(), j(3, "gpt-6-luna").id(), "a redelivery is the same job");
    assert_ne!(j(1, "gpt-6-luna").id(), j(1, "gpt-6.1-sol").id());
}

proptest! {
    #[test]
    fn any_document_round_trips(text in "\\PC{0,200}", r in 0i64..i64::MAX / 2, version in 1u32..10) {
        let mut d = doc(&text, r);
        d.version = version;
        d.id = d.id();
        let back = decode(&encode(&Record::Document(Box::new(d.clone())))).unwrap();
        prop_assert_eq!(back, Record::Document(Box::new(d)));
    }

    #[test]
    fn garbage_is_refused_not_trusted(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        // Either it decodes to a record whose re-encoding decodes equally, or it is refused; never a panic.
        if let Ok(r) = decode(&bytes) {
            prop_assert_eq!(decode(&encode(&r)).unwrap(), r);
        }
    }
}

//! End-to-end core tests: consent -> ingest -> embed -> hybrid search -> revoke.

use seeqora_core::embed::HashEmbedder;
use seeqora_core::record::{Record, SourceKind};
use seeqora_core::retrieve::{embed_pending, hybrid_search};
use seeqora_core::store::{Filters, Store, UpsertOutcome};
use seeqora_core::CoreError;
use std::collections::BTreeMap;

const DAY: i64 = 86_400_000;

fn rec(source: SourceKind, id: &str, ts: i64, text: &str, sender: Option<&str>) -> Record {
    let mut metadata = BTreeMap::new();
    if let Some(s) = sender {
        metadata.insert("sender".to_string(), s.to_string());
    }
    Record {
        source,
        item_id: id.into(),
        timestamp_ms: ts,
        text: text.into(),
        metadata,
        uri: format!("{}://{}", source.as_str(), id),
    }
}

fn seeded() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.grant(SourceKind::Notes).unwrap();
    s.grant(SourceKind::Messages).unwrap();
    s.upsert_record(&rec(SourceKind::Messages, "m1", 10 * DAY, "the wifi password is otter-42", Some("Dana"))).unwrap();
    s.upsert_record(&rec(SourceKind::Messages, "m2", 20 * DAY, "dentist appointment moved to friday", Some("Sam"))).unwrap();
    s.upsert_record(&rec(SourceKind::Notes, "n1", 15 * DAY, "Groceries\n\nRouter admin password is on the fridge", None)).unwrap();
    s
}

#[test]
fn ingestion_requires_consent() {
    let mut s = Store::open_in_memory().unwrap();
    let err = s
        .upsert_record(&rec(SourceKind::Photos, "p1", 0, "receipt", None))
        .unwrap_err();
    assert!(matches!(err, CoreError::NoConsent(_)));
}

#[test]
fn reindex_is_idempotent_and_incremental() {
    let mut s = seeded();
    let n1 = rec(SourceKind::Notes, "n1", 15 * DAY, "Groceries\n\nRouter admin password is on the fridge", None);
    assert_eq!(s.upsert_record(&n1).unwrap(), UpsertOutcome::Unchanged);

    let n1b = rec(SourceKind::Notes, "n1", 15 * DAY, "Groceries\n\nRouter password changed", None);
    assert_eq!(
        s.upsert_record(&n1b).unwrap(),
        UpsertOutcome::Indexed { written: 1, unchanged: 1, removed: 0 }
    );

    let n1c = rec(SourceKind::Notes, "n1", 15 * DAY, "Groceries", None);
    assert_eq!(
        s.upsert_record(&n1c).unwrap(),
        UpsertOutcome::Indexed { written: 0, unchanged: 1, removed: 1 }
    );
}

#[test]
fn unchanged_chunks_keep_their_embeddings() {
    let mut s = seeded();
    let e = HashEmbedder::new(64);
    assert_eq!(embed_pending(&mut s, &e).unwrap(), 4);
    assert_eq!(embed_pending(&mut s, &e).unwrap(), 0);

    // Change only the second paragraph of n1: exactly one chunk needs re-embedding.
    s.upsert_record(&rec(SourceKind::Notes, "n1", 15 * DAY, "Groceries\n\nnew text", None)).unwrap();
    assert_eq!(embed_pending(&mut s, &e).unwrap(), 1);
}

#[test]
fn hybrid_search_finds_and_cites() {
    let mut s = seeded();
    let e = HashEmbedder::new(128);
    embed_pending(&mut s, &e).unwrap();

    let r = hybrid_search(&s, Some(&e), "wifi password", &Filters::default(), 8).unwrap();
    assert!(r.degraded.is_none());
    let top = s.get_chunk(&r.hits[0].chunk_id).unwrap().unwrap();
    assert_eq!(top.item_id, "m1");
    assert_eq!(top.uri, "messages://m1");
    assert!(r.hits[0].dense_rank.is_some() && r.hits[0].keyword_rank.is_some());
}

#[test]
fn filters_apply_before_fusion() {
    let mut s = seeded();
    let e = HashEmbedder::new(128);
    embed_pending(&mut s, &e).unwrap();

    let only_notes = Filters { source: Some(SourceKind::Notes), ..Default::default() };
    let r = hybrid_search(&s, Some(&e), "password", &only_notes, 8).unwrap();
    assert!(!r.hits.is_empty());
    for h in &r.hits {
        assert_eq!(s.get_chunk(&h.chunk_id).unwrap().unwrap().source, SourceKind::Notes);
    }

    let by_sender = Filters { sender: Some("Sam".into()), ..Default::default() };
    let r = hybrid_search(&s, Some(&e), "password dentist", &by_sender, 8).unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(s.get_chunk(&r.hits[0].chunk_id).unwrap().unwrap().item_id, "m2");

    let window = Filters { from_ms: Some(18 * DAY), to_ms: Some(25 * DAY), ..Default::default() };
    let r = hybrid_search(&s, None, "password dentist", &window, 8).unwrap();
    assert_eq!(r.hits.len(), 1);
}

#[test]
fn missing_model_degrades_to_keyword_with_message() {
    let s = seeded();
    let r = hybrid_search(&s, None, "otter", &Filters::default(), 8).unwrap();
    assert!(r.degraded.is_some());
    assert_eq!(r.hits.len(), 1);
}

#[test]
fn revoke_deletes_everything_for_source_and_reports_counts() {
    let mut s = seeded();
    embed_pending(&mut s, &HashEmbedder::new(32)).unwrap();
    let report = s.revoke(SourceKind::Messages).unwrap();
    assert_eq!((report.items, report.chunks, report.embeddings), (2, 2, 2));
    assert!(!s.is_consented(SourceKind::Messages).unwrap());
    let r = hybrid_search(&s, None, "otter", &Filters::default(), 8).unwrap();
    assert!(r.hits.is_empty());
    assert_eq!(s.count_chunks().unwrap(), 2);
}

#[test]
fn delete_everything_wipes_index() {
    let mut s = seeded();
    s.delete_everything().unwrap();
    assert_eq!(s.count_chunks().unwrap(), 0);
    assert!(!s.is_consented(SourceKind::Notes).unwrap());
}

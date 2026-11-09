//! The facade the Swift and Kotlin shells call (T6). Shells hand in records
//! from their read-only adapters and render what comes back; they never touch
//! the store or retrieval directly.
//!
//! Until the embedding model is chosen (W4), the app ships without one, so
//! search runs keyword-only and returns the degraded notice for the shell to
//! show (T9).

use crate::error::CoreError;
use crate::record::{Record, SourceKind};
use crate::retrieve::hybrid_search;
use crate::store::{Filters, Store, UpsertOutcome};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Errors the shell can show as a plain message.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum SeeqoraError {
    /// The user has not switched this source on (T3).
    #[error("{0}")]
    NoConsent(String),
    #[error("{0}")]
    Storage(String),
}

impl From<CoreError> for SeeqoraError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NoConsent(_) => SeeqoraError::NoConsent(e.to_string()),
            other => SeeqoraError::Storage(other.to_string()),
        }
    }
}

type FfiResult<T> = std::result::Result<T, SeeqoraError>;

/// One item from a platform adapter.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SourceRecord {
    pub source: SourceKind,
    pub item_id: String,
    pub timestamp_ms: i64,
    pub text: String,
    pub metadata: HashMap<String, String>,
    pub uri: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SourceStatus {
    pub source: SourceKind,
    pub granted: bool,
    pub items: i64,
}

/// What switching a source off removed (T3).
#[derive(Debug, Clone, uniffi::Record)]
pub struct RevokeSummary {
    pub items: i64,
    pub chunks: i64,
    pub embeddings: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct IndexSummary {
    pub indexed: i64,
    pub unchanged: i64,
}

/// One cited result (T2). `uri` is the tap-through to the original.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchHit {
    pub chunk_id: String,
    pub source: SourceKind,
    pub item_id: String,
    pub text: String,
    pub uri: String,
    pub timestamp_ms: i64,
    pub metadata: HashMap<String, String>,
    pub score: f64,
    pub dense_rank: Option<i32>,
    pub keyword_rank: Option<i32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResults {
    pub hits: Vec<SearchHit>,
    /// Set when search ran in a reduced mode; the shell must show it (T9).
    pub degraded: Option<String>,
}

#[derive(uniffi::Object)]
pub struct Engine {
    store: Mutex<Store>,
}

impl Engine {
    fn store(&self) -> MutexGuard<'_, Store> {
        // A panic elsewhere must not lock the user out of their index.
        self.store.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[uniffi::export]
impl Engine {
    /// Open (or create) the index file at `path`.
    #[uniffi::constructor]
    pub fn open(path: String) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self {
            store: Mutex::new(Store::open(path)?),
        }))
    }

    #[uniffi::constructor]
    pub fn in_memory() -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self {
            store: Mutex::new(Store::open_in_memory()?),
        }))
    }

    pub fn sources(&self) -> FfiResult<Vec<SourceStatus>> {
        let s = self.store();
        SourceKind::ALL
            .iter()
            .map(|&k| -> FfiResult<SourceStatus> {
                Ok(SourceStatus {
                    source: k,
                    granted: s.is_consented(k)?,
                    items: s.count_items(k)? as i64,
                })
            })
            .collect()
    }

    pub fn grant(&self, source: SourceKind) -> FfiResult<()> {
        Ok(self.store().grant(source)?)
    }

    pub fn revoke(&self, source: SourceKind) -> FfiResult<RevokeSummary> {
        let r = self.store().revoke(source)?;
        Ok(RevokeSummary {
            items: r.items as i64,
            chunks: r.chunks as i64,
            embeddings: r.embeddings as i64,
        })
    }

    pub fn delete_everything(&self) -> FfiResult<()> {
        Ok(self.store().delete_everything()?)
    }

    /// Index records. Fails with `NoConsent` if any record's source is off.
    pub fn index(&self, records: Vec<SourceRecord>) -> FfiResult<IndexSummary> {
        let mut s = self.store();
        let mut out = IndexSummary {
            indexed: 0,
            unchanged: 0,
        };
        for r in records {
            let rec = Record {
                source: r.source,
                item_id: r.item_id,
                timestamp_ms: r.timestamp_ms,
                text: r.text,
                metadata: r.metadata.into_iter().collect(),
                uri: r.uri,
            };
            match s.upsert_record(&rec)? {
                UpsertOutcome::Unchanged => out.unchanged += 1,
                UpsertOutcome::Indexed { .. } => out.indexed += 1,
            }
        }
        Ok(out)
    }

    /// Item ids already indexed for a source, so adapters can skip costly
    /// re-extraction (photo OCR) of items the core already has (T5).
    pub fn indexed_ids(&self, source: SourceKind) -> FfiResult<Vec<String>> {
        Ok(self.store().item_ids(source)?)
    }

    /// After a full read of a source, remove items that are gone from the
    /// device. Returns how many were removed.
    pub fn retain_only(&self, source: SourceKind, item_ids: Vec<String>) -> FfiResult<i64> {
        Ok(self.store().retain_items(source, &item_ids)? as i64)
    }

    /// Search switched-on sources. Revoked sources have no rows left, so they
    /// can never appear.
    pub fn search(&self, query: String, top_n: u32) -> FfiResult<SearchResults> {
        let s = self.store();
        let r = hybrid_search(&s, None, &query, &Filters::default(), top_n as usize)?;
        let mut hits = Vec::with_capacity(r.hits.len());
        for h in r.hits {
            if let Some(c) = s.get_chunk(&h.chunk_id)? {
                hits.push(SearchHit {
                    chunk_id: c.chunk_id,
                    source: c.source,
                    item_id: c.item_id,
                    text: c.text,
                    uri: c.uri,
                    timestamp_ms: c.timestamp_ms,
                    metadata: c.metadata.into_iter().collect(),
                    score: h.score,
                    dense_rank: h.dense_rank.map(|x| x as i32),
                    keyword_rank: h.keyword_rank.map(|x| x as i32),
                });
            }
        }
        Ok(SearchResults {
            hits,
            degraded: r.degraded,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(source: SourceKind, id: &str, text: &str) -> SourceRecord {
        SourceRecord {
            source,
            item_id: id.into(),
            timestamp_ms: 1_760_000_000_000,
            text: text.into(),
            metadata: HashMap::from([("sender".to_string(), "Dana".to_string())]),
            uri: format!("{}://{id}", source.as_str()),
        }
    }

    #[test]
    fn consent_index_search_revoke() {
        let e = Engine::in_memory().unwrap();
        let wifi = rec(SourceKind::Messages, "m1", "cabin wifi password is Tamarack");
        assert!(matches!(
            e.index(vec![wifi.clone()]),
            Err(SeeqoraError::NoConsent(_))
        ));

        e.grant(SourceKind::Messages).unwrap();
        assert_eq!(e.index(vec![wifi.clone()]).unwrap().indexed, 1);
        assert_eq!(e.index(vec![wifi]).unwrap().unchanged, 1);

        // Prefix match on the last word while typing.
        assert_eq!(e.search("cabin wif".into(), 10).unwrap().hits[0].item_id, "m1");

        let r = e.search("wifi password".into(), 10).unwrap();
        assert!(r.degraded.is_some(), "keyword-only must be announced");
        assert_eq!(r.hits[0].item_id, "m1");
        assert_eq!(r.hits[0].uri, "messages://m1");
        assert_eq!(r.hits[0].metadata.get("sender").map(String::as_str), Some("Dana"));

        let status = e.sources().unwrap();
        let m = status.iter().find(|s| s.source == SourceKind::Messages).unwrap();
        assert!(m.granted);
        assert_eq!(m.items, 1);

        e.index(vec![rec(SourceKind::Messages, "m2", "dinner at eight")]).unwrap();
        assert_eq!(e.indexed_ids(SourceKind::Messages).unwrap(), vec!["m1", "m2"]);
        assert_eq!(e.retain_only(SourceKind::Messages, vec!["m1".into()]).unwrap(), 1);
        assert!(e.search("dinner".into(), 10).unwrap().hits.is_empty());

        assert_eq!(e.revoke(SourceKind::Messages).unwrap().items, 1);
        assert!(e.search("wifi".into(), 10).unwrap().hits.is_empty());
    }
}

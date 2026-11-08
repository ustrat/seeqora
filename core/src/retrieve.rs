//! Hybrid retrieval: dense cosine top-k and BM25 top-k, fused by reciprocal
//! rank fusion with a fixed, logged formula (T7). Metadata filters are applied
//! inside each retriever, before fusion.
//!
//! `score(chunk) = sum over retrievers of 1 / (RRF_K + rank)`, rank starting at 1.
//! Ties break by chunk id, so results are deterministic.

use crate::embed::Embedder;
use crate::error::{CoreError, Result};
use crate::store::{Filters, Store};
use std::collections::HashMap;

pub const DENSE_K: usize = 40;
pub const KEYWORD_K: usize = 40;
pub const RRF_K: f64 = 60.0;
/// Chunks embedded per indexer wake-up (Battery doctrine: batch, don't trickle).
pub const EMBED_BATCH: usize = 256;

/// One retrieved chunk with everything the debug view needs to explain it.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub chunk_id: String,
    pub score: f64,
    pub dense_rank: Option<usize>,
    pub keyword_rank: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalResult {
    pub hits: Vec<Hit>,
    /// Set when search ran in a reduced mode; the shell shows it (T9).
    pub degraded: Option<String>,
}

/// Reciprocal rank fusion of two ranked id lists (best first).
pub fn rrf_fuse(dense: &[String], keyword: &[String]) -> Vec<Hit> {
    let mut map: HashMap<&str, Hit> = HashMap::new();
    for (i, id) in dense.iter().enumerate() {
        let h = map.entry(id.as_str()).or_insert_with(|| Hit {
            chunk_id: id.clone(),
            score: 0.0,
            dense_rank: None,
            keyword_rank: None,
        });
        if h.dense_rank.is_none() {
            h.dense_rank = Some(i + 1);
            h.score += 1.0 / (RRF_K + (i + 1) as f64);
        }
    }
    for (i, id) in keyword.iter().enumerate() {
        let h = map.entry(id.as_str()).or_insert_with(|| Hit {
            chunk_id: id.clone(),
            score: 0.0,
            dense_rank: None,
            keyword_rank: None,
        });
        if h.keyword_rank.is_none() {
            h.keyword_rank = Some(i + 1);
            h.score += 1.0 / (RRF_K + (i + 1) as f64);
        }
    }
    let mut hits: Vec<Hit> = map.into_values().collect();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });
    hits
}

/// Embed up to one batch of pending chunks. Returns how many were embedded.
pub fn embed_pending(store: &mut Store, embedder: &dyn Embedder) -> Result<usize> {
    let pending = store.chunks_needing_embedding(embedder.model_id(), EMBED_BATCH)?;
    if pending.is_empty() {
        return Ok(0);
    }
    let texts: Vec<&str> = pending.iter().map(|(_, t)| t.as_str()).collect();
    let vecs = embedder.embed(&texts)?;
    if vecs.len() != pending.len() {
        return Err(CoreError::Embedding(format!(
            "expected {} vectors, got {}",
            pending.len(),
            vecs.len()
        )));
    }
    for ((id, _), v) in pending.iter().zip(vecs) {
        store.put_embedding(id, embedder.model_id(), &v)?;
    }
    Ok(pending.len())
}

/// Hybrid search. If no embedder is available (model missing), it falls back
/// to keyword-only and says so instead of failing (T9).
pub fn hybrid_search(
    store: &Store,
    embedder: Option<&dyn Embedder>,
    query: &str,
    filters: &Filters,
    top_n: usize,
) -> Result<RetrievalResult> {
    let keyword: Vec<String> = store
        .keyword_search(query, filters, KEYWORD_K)?
        .into_iter()
        .map(|(id, _)| id)
        .collect();

    let (dense, degraded) = match embedder {
        Some(e) => {
            let qv = e
                .embed(&[query])?
                .into_iter()
                .next()
                .ok_or_else(|| CoreError::Embedding("empty query embedding".into()))?;
            let d = store
                .dense_search(&qv, e.model_id(), filters, DENSE_K)?
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            (d, None)
        }
        None => (
            vec![],
            Some("Embedding model unavailable: showing keyword matches only.".to_string()),
        ),
    };

    let mut hits = rrf_fuse(&dense, &keyword);
    hits.truncate(top_n);
    Ok(RetrievalResult { hits, degraded })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rrf_rewards_agreement_and_explains_ranks() {
        let hits = rrf_fuse(&ids(&["a", "b", "c"]), &ids(&["b", "d"]));
        assert_eq!(hits[0].chunk_id, "b");
        assert_eq!(hits[0].dense_rank, Some(2));
        assert_eq!(hits[0].keyword_rank, Some(1));
        let expected = 1.0 / 62.0 + 1.0 / 61.0;
        assert!((hits[0].score - expected).abs() < 1e-12);
    }

    #[test]
    fn rrf_is_deterministic_on_ties() {
        let hits = rrf_fuse(&ids(&["z"]), &ids(&["a"]));
        assert_eq!(hits[0].chunk_id, "a");
        assert_eq!(hits[1].chunk_id, "z");
    }

    #[test]
    fn rrf_ignores_duplicate_ids_within_a_list() {
        let hits = rrf_fuse(&ids(&["a", "a"]), &[]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].dense_rank, Some(1));
    }
}

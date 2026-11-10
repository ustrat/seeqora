//! Hybrid retrieval: dense cosine top-k and BM25 top-k, fused by reciprocal
//! rank fusion with a fixed, logged formula (T7). Metadata filters are applied
//! inside each retriever, before fusion.
//!
//! `score(chunk) = sum over retrievers of 1 / (RRF_K + rank)`, rank starting at 1.
//! Ties break by chunk id, so results are deterministic.

use crate::embed::Embedder;
use crate::error::{CoreError, Result};
use crate::record::SourceKind;
use crate::store::{Filters, Store};
use crate::when::{self, Order};
use std::collections::{HashMap, HashSet};

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
            let qv = e.embed_query(query)?;
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

/// Candidates considered before time-based ordering.
pub const POOL: usize = 40;
/// For "next"/"upcoming": an upcoming item jumps the queue if its score is at
/// least this share of the best one, so a weak match can't lead.
const UPCOMING_MIN_SHARE: f64 = 0.5;
/// For "last"/"latest": the items reordered newest-first.
const LATEST_MIN_SHARE: f64 = 0.75;

/// Search for a question asked at `now_ms`, reading its time words (W5).
///
/// - A window ("last Thursday", "next week", "in November", "yesterday"):
///   matches inside it first, then the rest of the window in time order
///   (they may share no words with the question), then everything else.
/// - "next"/"upcoming": strong matches about today or later, soonest first.
///   A reminder counts by the date it mentions, not the day it was sent.
///   Then the rest, newest first among strong matches.
/// - "last"/"latest": strong matches newest first.
///
/// Nothing is dropped, only reordered, so a wrong reading of the question
/// costs rank, not the answer. The results list and the answers both use it (T6).
pub fn search_for(
    store: &Store,
    embedder: Option<&dyn Embedder>,
    question: &str,
    now_ms: i64,
    utc_offset_minutes: i32,
    top_n: usize,
) -> Result<RetrievalResult> {
    let off = utc_offset_minutes;
    let tq = when::parse_question(question, now_ms, off);
    let base = hybrid_search(store, embedder, question, &Filters::default(), POOL)?;
    let degraded = base.degraded;

    let mut hits = if tq.has_window() {
        let f = Filters {
            source: tq.source,
            from_ms: tq.from_ms,
            to_ms: tq.to_ms,
            sender: None,
        };
        let mut out = hybrid_search(store, embedder, question, &f, POOL)?.hits;
        let mut seen: HashSet<String> = out.iter().map(|h| h.chunk_id.clone()).collect();
        for id in store.chunks_in_window(&f, false, POOL)? {
            if seen.insert(id.clone()) {
                out.push(Hit { chunk_id: id, score: 0.0, dense_rank: None, keyword_rank: None });
            }
        }
        out.extend(base.hits.into_iter().filter(|h| !seen.contains(&h.chunk_id)));
        out
    } else {
        match tq.order {
            Order::Relevance => base.hits,
            Order::Latest => newest_strong_first(store, base.hits, LATEST_MIN_SHARE)?,
            Order::Soonest if tq.source == Some(SourceKind::Calendar) => {
                // "What's next on my calendar?": upcoming events in time order.
                let f = Filters {
                    source: Some(SourceKind::Calendar),
                    from_ms: Some(when::day_start_ms(when::local_day(now_ms, off), off)),
                    ..Filters::default()
                };
                let mut out: Vec<Hit> = Vec::new();
                let mut seen: HashSet<String> = HashSet::new();
                for id in store.chunks_in_window(&f, false, POOL)? {
                    if seen.insert(id.clone()) {
                        out.push(Hit { chunk_id: id, score: 0.0, dense_rank: None, keyword_rank: None });
                    }
                }
                out.extend(base.hits.into_iter().filter(|h| !seen.contains(&h.chunk_id)));
                out
            }
            Order::Soonest => soonest_first(store, embedder, question, base.hits, now_ms, off)?,
        }
    };
    hits.truncate(top_n);
    Ok(RetrievalResult { hits, degraded })
}

fn best_score(hits: &[Hit]) -> f64 {
    hits.iter().map(|h| h.score).fold(0.0, f64::max)
}

fn by_score(a: &Hit, b: &Hit) -> std::cmp::Ordering {
    b.score.total_cmp(&a.score).then_with(|| a.chunk_id.cmp(&b.chunk_id))
}

/// Strong matches (score >= `share` of the best) newest first, then the rest by score.
fn newest_strong_first(store: &Store, hits: Vec<Hit>, share: f64) -> Result<Vec<Hit>> {
    let best = best_score(&hits);
    let mut strong: Vec<(i64, Hit)> = Vec::new();
    let mut rest: Vec<Hit> = Vec::new();
    for h in hits {
        if best > 0.0 && h.score >= share * best {
            let t = store.get_chunk(&h.chunk_id)?.map_or(i64::MIN, |c| c.timestamp_ms);
            strong.push((t, h));
        } else {
            rest.push(h);
        }
    }
    strong.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| by_score(&a.1, &b.1)));
    rest.sort_by(by_score);
    Ok(strong.into_iter().map(|(_, h)| h).chain(rest).collect())
}

fn soonest_first(
    store: &Store,
    embedder: Option<&dyn Embedder>,
    question: &str,
    base: Vec<Hit>,
    now_ms: i64,
    off: i32,
) -> Result<Vec<Hit>> {
    let today = when::local_day(now_ms, off);
    let today_start = when::day_start_ms(today, off);
    // Future events and recent reminders may not make the general top 40
    // when years of older items share the question's words.
    let mut pool = base;
    let mut seen: HashSet<String> = pool.iter().map(|h| h.chunk_id.clone()).collect();
    let extra = [
        Filters {
            source: Some(SourceKind::Calendar),
            from_ms: Some(today_start),
            ..Filters::default()
        },
        Filters {
            from_ms: Some(today_start - 60 * when::DAY_MS),
            ..Filters::default()
        },
    ];
    for f in &extra {
        for h in hybrid_search(store, embedder, question, f, POOL / 2)?.hits {
            if seen.insert(h.chunk_id.clone()) {
                pool.push(h);
            }
        }
    }

    let best = best_score(&pool);
    let mut upcoming: Vec<(i64, Hit)> = Vec::new();
    let mut rest: Vec<Hit> = Vec::new();
    for h in pool {
        let next = if best > 0.0 && h.score >= UPCOMING_MIN_SHARE * best {
            store.get_chunk(&h.chunk_id)?.and_then(|c| {
                when::event_days(c.source, c.timestamp_ms, &c.text, off)
                    .into_iter()
                    .find(|&d| d >= today)
            })
        } else {
            None
        };
        match next {
            Some(d) => upcoming.push((d, h)),
            None => rest.push(h),
        }
    }
    upcoming.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| by_score(&a.1, &b.1)));
    // Nothing upcoming among them: the most recent strong matches lead, so
    // the model can say "none upcoming; the last one was …".
    let rest = newest_strong_first(store, rest, UPCOMING_MIN_SHARE)?;
    Ok(upcoming.into_iter().map(|(_, h)| h).chain(rest).collect())
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

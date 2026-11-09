//! The single on-device datastore: one SQLite file per user holding `sources`
//! (consent), `items`, `chunks`, `embeddings`, and an FTS5 keyword index.
//! There is no second datastore.
//!
//! Phase 0: vectors are BLOBs searched by brute-force cosine. sqlite-vec swaps
//! in at W4 behind the same API.

use crate::chunk::{chunk_record, Chunk};
use crate::embed::cosine;
use crate::error::{CoreError, Result};
use crate::record::{Record, SourceKind};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
    source        TEXT PRIMARY KEY,
    consented     INTEGER NOT NULL DEFAULT 0,
    consented_at  INTEGER,
    revoked_at    INTEGER
);

CREATE TABLE IF NOT EXISTS items (
    source        TEXT NOT NULL REFERENCES sources(source),
    item_id       TEXT NOT NULL,
    timestamp_ms  INTEGER NOT NULL,
    uri           TEXT NOT NULL,
    metadata      TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    PRIMARY KEY (source, item_id)
);
CREATE INDEX IF NOT EXISTS items_by_time ON items(source, timestamp_ms);

CREATE TABLE IF NOT EXISTS chunks (
    chunk_id   TEXT PRIMARY KEY,
    source     TEXT NOT NULL,
    item_id    TEXT NOT NULL,
    ordinal    INTEGER NOT NULL,
    text       TEXT NOT NULL,
    text_hash  TEXT NOT NULL,
    start_ms   INTEGER,
    end_ms     INTEGER,
    FOREIGN KEY (source, item_id) REFERENCES items(source, item_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS chunks_by_item ON chunks(source, item_id);

-- Excluded from backups; rebuilds from items.
CREATE TABLE IF NOT EXISTS embeddings (
    chunk_id  TEXT PRIMARY KEY REFERENCES chunks(chunk_id) ON DELETE CASCADE,
    model_id  TEXT NOT NULL,
    dims      INTEGER NOT NULL,
    vector    BLOB NOT NULL
);

-- Porter stemming so "changed" matches "change" and "plants" matches "Planting".
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    text,
    chunk_id UNINDEXED,
    tokenize = 'porter unicode61 remove_diacritics 2'
);
"#;

/// Metadata filters, applied *before* fusion (T7).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filters {
    pub source: Option<SourceKind>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    /// Matches `metadata.sender`.
    pub sender: Option<String>,
}

const FILTER_SQL: &str = "
    AND (?2 IS NULL OR c.source = ?2)
    AND (?3 IS NULL OR i.timestamp_ms >= ?3)
    AND (?4 IS NULL OR i.timestamp_ms <= ?4)
    AND (?5 IS NULL OR json_extract(i.metadata, '$.sender') = ?5)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// Content hash matched; nothing was touched.
    Unchanged,
    Indexed {
        written: usize,
        unchanged: usize,
        removed: usize,
    },
}

/// What a revoke deleted, shown to the user (T3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RevokeReport {
    pub items: usize,
    pub chunks: usize,
    pub embeddings: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredChunk {
    pub chunk_id: String,
    pub source: SourceKind,
    pub item_id: String,
    pub text: String,
    pub uri: String,
    pub timestamp_ms: i64,
    /// Item metadata (sender, title, filename...) for the citation label.
    pub metadata: BTreeMap<String, String>,
}

pub struct Store {
    conn: Connection,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn blob_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]))
        .collect()
}

/// Turn free text into a safe FTS5 OR-query of quoted terms. The last term is
/// also a prefix match, so search-as-you-type finds "wifi" while "wif" is typed.
fn fts_query(q: &str) -> Option<String> {
    let words: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect();
    let last = words.len().saturating_sub(1);
    let terms: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if i == last && t.chars().count() >= 2 {
                format!("\"{t}\" OR \"{t}\"*")
            } else {
                format!("\"{t}\"")
            }
        })
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        // WAL + NORMAL: one fsync per checkpoint instead of per item, so
        // indexing thousands of messages doesn't hammer flash (T5).
        let _mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    // ---- consent (T3) -------------------------------------------------------

    pub fn is_consented(&self, source: SourceKind) -> Result<bool> {
        let v: Option<i64> = self
            .conn
            .query_row(
                "SELECT consented FROM sources WHERE source = ?1",
                params![source.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v == Some(1))
    }

    pub fn grant(&mut self, source: SourceKind) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sources (source, consented, consented_at, revoked_at)
             VALUES (?1, 1, ?2, NULL)
             ON CONFLICT(source) DO UPDATE SET consented = 1, consented_at = ?2, revoked_at = NULL",
            params![source.as_str(), now_ms()],
        )?;
        Ok(())
    }

    /// Revoke a source and delete its items, chunks, keyword rows and embeddings.
    pub fn revoke(&mut self, source: SourceKind) -> Result<RevokeReport> {
        let s = source.as_str();
        let tx = self.conn.transaction()?;
        let count = |sql: &str| -> Result<usize> {
            Ok(tx.query_row(sql, params![s], |r| r.get::<_, i64>(0))? as usize)
        };
        let report = RevokeReport {
            items: count("SELECT COUNT(*) FROM items WHERE source = ?1")?,
            chunks: count("SELECT COUNT(*) FROM chunks WHERE source = ?1")?,
            embeddings: count(
                "SELECT COUNT(*) FROM embeddings e JOIN chunks c ON c.chunk_id = e.chunk_id
                 WHERE c.source = ?1",
            )?,
        };
        tx.execute(
            "DELETE FROM chunks_fts WHERE chunk_id IN (SELECT chunk_id FROM chunks WHERE source = ?1)",
            params![s],
        )?;
        tx.execute(
            "DELETE FROM embeddings WHERE chunk_id IN (SELECT chunk_id FROM chunks WHERE source = ?1)",
            params![s],
        )?;
        tx.execute("DELETE FROM chunks WHERE source = ?1", params![s])?;
        tx.execute("DELETE FROM items WHERE source = ?1", params![s])?;
        tx.execute(
            "INSERT INTO sources (source, consented, revoked_at) VALUES (?1, 0, ?2)
             ON CONFLICT(source) DO UPDATE SET consented = 0, revoked_at = ?2",
            params![s, now_ms()],
        )?;
        tx.commit()?;
        Ok(report)
    }

    /// "Delete everything": wipe all indexed data and consent state.
    pub fn delete_everything(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM chunks_fts; DELETE FROM embeddings; DELETE FROM chunks;
             DELETE FROM items; DELETE FROM sources;",
        )?;
        Ok(())
    }

    // ---- ingestion ----------------------------------------------------------

    /// Idempotent upsert. Unchanged items are skipped entirely; within a changed
    /// item only chunks whose text changed are rewritten (and lose their
    /// embedding), so unchanged chunks are never re-embedded (T5).
    pub fn upsert_record(&mut self, rec: &Record) -> Result<UpsertOutcome> {
        if !self.is_consented(rec.source)? {
            return Err(CoreError::NoConsent(rec.source.as_str().to_string()));
        }
        let src = rec.source.as_str();
        let hash = rec.content_hash();
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT content_hash FROM items WHERE source = ?1 AND item_id = ?2",
                params![src, rec.item_id],
                |r| r.get(0),
            )
            .optional()?;
        if existing.as_deref() == Some(hash.as_str()) {
            return Ok(UpsertOutcome::Unchanged);
        }

        let new_chunks: Vec<Chunk> = chunk_record(rec);
        let metadata = serde_json::to_string(&rec.metadata)?;

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO items (source, item_id, timestamp_ms, uri, metadata, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(source, item_id) DO UPDATE SET
                timestamp_ms = excluded.timestamp_ms, uri = excluded.uri,
                metadata = excluded.metadata, content_hash = excluded.content_hash",
            params![src, rec.item_id, rec.timestamp_ms, rec.uri, metadata, hash],
        )?;

        let old: HashMap<String, String> = {
            let mut stmt = tx.prepare(
                "SELECT chunk_id, text_hash FROM chunks WHERE source = ?1 AND item_id = ?2",
            )?;
            let rows = stmt.query_map(params![src, rec.item_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.collect::<std::result::Result<_, _>>()?
        };

        let (mut written, mut unchanged) = (0usize, 0usize);
        for c in &new_chunks {
            let th = c.text_hash();
            if old.get(&c.chunk_id) == Some(&th) {
                unchanged += 1;
                continue;
            }
            tx.execute("DELETE FROM chunks_fts WHERE chunk_id = ?1", params![c.chunk_id])?;
            tx.execute("DELETE FROM embeddings WHERE chunk_id = ?1", params![c.chunk_id])?;
            tx.execute("DELETE FROM chunks WHERE chunk_id = ?1", params![c.chunk_id])?;
            tx.execute(
                "INSERT INTO chunks (chunk_id, source, item_id, ordinal, text, text_hash, start_ms, end_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![c.chunk_id, src, c.item_id, c.ordinal, c.text, th, c.start_ms, c.end_ms],
            )?;
            tx.execute(
                "INSERT INTO chunks_fts (text, chunk_id) VALUES (?1, ?2)",
                params![c.text, c.chunk_id],
            )?;
            written += 1;
        }

        let keep: std::collections::HashSet<&str> =
            new_chunks.iter().map(|c| c.chunk_id.as_str()).collect();
        let mut removed = 0usize;
        for id in old.keys().filter(|id| !keep.contains(id.as_str())) {
            tx.execute("DELETE FROM chunks_fts WHERE chunk_id = ?1", params![id])?;
            tx.execute("DELETE FROM embeddings WHERE chunk_id = ?1", params![id])?;
            tx.execute("DELETE FROM chunks WHERE chunk_id = ?1", params![id])?;
            removed += 1;
        }
        tx.commit()?;

        Ok(UpsertOutcome::Indexed {
            written,
            unchanged,
            removed,
        })
    }

    // ---- embeddings ---------------------------------------------------------

    /// Chunks with no vector for `model_id`, up to `limit` (batch size 256).
    pub fn chunks_needing_embedding(
        &self,
        model_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.chunk_id, c.text FROM chunks c
             LEFT JOIN embeddings e ON e.chunk_id = c.chunk_id AND e.model_id = ?1
             WHERE e.chunk_id IS NULL
             ORDER BY c.chunk_id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![model_id, limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn put_embedding(&mut self, chunk_id: &str, model_id: &str, v: &[f32]) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO embeddings (chunk_id, model_id, dims, vector)
             VALUES (?1, ?2, ?3, ?4)",
            params![chunk_id, model_id, v.len() as i64, vec_to_blob(v)],
        )?;
        Ok(())
    }

    // ---- search -------------------------------------------------------------

    fn filter_params(f: &Filters) -> (Option<&'static str>, Option<i64>, Option<i64>, Option<&str>) {
        (
            f.source.map(|s| s.as_str()),
            f.from_ms,
            f.to_ms,
            f.sender.as_deref(),
        )
    }

    /// BM25 keyword search. Returns `(chunk_id, score)`, best first; higher is better.
    pub fn keyword_search(&self, query: &str, filters: &Filters, k: usize) -> Result<Vec<(String, f64)>> {
        let Some(q) = fts_query(query) else {
            return Ok(vec![]);
        };
        let sql = format!(
            "SELECT chunks_fts.chunk_id, bm25(chunks_fts) AS score
             FROM chunks_fts
             JOIN chunks c ON c.chunk_id = chunks_fts.chunk_id
             JOIN items i ON i.source = c.source AND i.item_id = c.item_id
             WHERE chunks_fts MATCH ?1 {FILTER_SQL}
             ORDER BY score ASC, chunks_fts.chunk_id ASC
             LIMIT ?6"
        );
        let (s, from, to, sender) = Self::filter_params(filters);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![q, s, from, to, sender, k as i64], |r| {
            Ok((r.get::<_, String>(0)?, -r.get::<_, f64>(1)?))
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Dense cosine search (brute force in Phase 0). Best first, ties by chunk id.
    pub fn dense_search(
        &self,
        query_vec: &[f32],
        model_id: &str,
        filters: &Filters,
        k: usize,
    ) -> Result<Vec<(String, f64)>> {
        let sql = format!(
            "SELECT e.chunk_id, e.vector
             FROM embeddings e
             JOIN chunks c ON c.chunk_id = e.chunk_id
             JOIN items i ON i.source = c.source AND i.item_id = c.item_id
             WHERE e.model_id = ?1 {FILTER_SQL}"
        );
        let (s, from, to, sender) = Self::filter_params(filters);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![model_id, s, from, to, sender], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut scored: Vec<(String, f64)> = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            scored.push((id, cosine(query_vec, &blob_to_vec(&blob)) as f64));
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scored.truncate(k);
        Ok(scored)
    }

    /// Resolve a chunk for citation tap-through (T2).
    pub fn get_chunk(&self, chunk_id: &str) -> Result<Option<StoredChunk>> {
        Ok(self
            .conn
            .query_row(
                "SELECT c.chunk_id, c.source, c.item_id, c.text, i.uri, i.timestamp_ms, i.metadata
                 FROM chunks c JOIN items i ON i.source = c.source AND i.item_id = c.item_id
                 WHERE c.chunk_id = ?1",
                params![chunk_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?
            .and_then(|(chunk_id, src, item_id, text, uri, timestamp_ms, meta)| {
                // Metadata is written by upsert_record; unreadable metadata only
                // costs the label, never the result (T9).
                let metadata: BTreeMap<String, String> =
                    serde_json::from_str(&meta).unwrap_or_default();
                SourceKind::parse(&src).map(|source| StoredChunk {
                    chunk_id,
                    source,
                    item_id,
                    text,
                    uri,
                    timestamp_ms,
                    metadata,
                })
            }))
    }

    /// Ids of indexed items for one source.
    pub fn item_ids(&self, source: SourceKind) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_id FROM items WHERE source = ?1 ORDER BY item_id")?;
        let rows = stmt.query_map(params![source.as_str()], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// After a full read of a source, drop items that no longer exist on the
    /// device (deleted messages, removed files). Returns how many were removed.
    pub fn retain_items(&mut self, source: SourceKind, keep: &[String]) -> Result<usize> {
        let keep: std::collections::HashSet<&str> = keep.iter().map(String::as_str).collect();
        let gone: Vec<String> = self
            .item_ids(source)?
            .into_iter()
            .filter(|id| !keep.contains(id.as_str()))
            .collect();
        let s = source.as_str();
        let tx = self.conn.transaction()?;
        for id in &gone {
            tx.execute(
                "DELETE FROM chunks_fts WHERE chunk_id IN
                   (SELECT chunk_id FROM chunks WHERE source = ?1 AND item_id = ?2)",
                params![s, id],
            )?;
            tx.execute(
                "DELETE FROM embeddings WHERE chunk_id IN
                   (SELECT chunk_id FROM chunks WHERE source = ?1 AND item_id = ?2)",
                params![s, id],
            )?;
            tx.execute(
                "DELETE FROM chunks WHERE source = ?1 AND item_id = ?2",
                params![s, id],
            )?;
            tx.execute(
                "DELETE FROM items WHERE source = ?1 AND item_id = ?2",
                params![s, id],
            )?;
        }
        tx.commit()?;
        Ok(gone.len())
    }

    /// Indexed items for one source (shown next to its consent toggle).
    pub fn count_items(&self, source: SourceKind) -> Result<usize> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM items WHERE source = ?1",
            params![source.as_str()],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }

    pub fn count_chunks(&self) -> Result<usize> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get::<_, i64>(0))? as usize)
    }
}

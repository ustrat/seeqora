//! Chunking by natural unit, not fixed tokens.
//!
//! - One message, one calendar event, one contact, one photo caption + OCR block
//!   each become a single chunk.
//! - Notes are split per paragraph.
//! - Long documents are split at headings, then into windows of about 300 tokens
//!   with 15% overlap.
//!
//! Chunk ids are derived from `(source, item_id, ordinal)`, so re-indexing is
//! idempotent (T4, T5).
//!
//! Token counts are a whitespace estimate until the embedding model's tokenizer
//! lands (build plan W2; see the decision log).

use crate::record::{Record, SourceKind};
use sha2::{Digest, Sha256};

pub const DOC_WINDOW_TOKENS: usize = 300;
/// 15% of 300.
pub const DOC_OVERLAP_TOKENS: usize = 45;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub chunk_id: String,
    pub source: SourceKind,
    pub item_id: String,
    pub ordinal: u32,
    pub text: String,
    /// Media only (Phase 2): position in the file this chunk cites.
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

impl Chunk {
    /// Hash of the chunk text; a chunk is re-embedded only when this changes.
    pub fn text_hash(&self) -> String {
        hex::encode(Sha256::digest(self.text.as_bytes()))
    }
}

/// Stable chunk id derived from the source item and the chunk's position.
pub fn chunk_id(source: SourceKind, item_id: &str, ordinal: u32) -> String {
    let mut h = Sha256::new();
    h.update(source.as_str().as_bytes());
    h.update([0u8]);
    h.update(item_id.as_bytes());
    h.update([0u8]);
    h.update(ordinal.to_le_bytes());
    hex::encode(&h.finalize()[..16])
}

/// Split a record into chunks according to its source's natural unit.
pub fn chunk_record(rec: &Record) -> Vec<Chunk> {
    let pieces: Vec<String> = match rec.source {
        SourceKind::Notes => split_paragraphs(&rec.text)
            .into_iter()
            .flat_map(|p| window_text(&p, DOC_WINDOW_TOKENS, DOC_OVERLAP_TOKENS))
            .collect(),
        SourceKind::Files | SourceKind::Email => split_sections(&rec.text)
            .into_iter()
            .flat_map(|s| window_text(&s, DOC_WINDOW_TOKENS, DOC_OVERLAP_TOKENS))
            .collect(),
        // Natural single units. Media transcript windows (30-60 s) land in Phase 2.
        SourceKind::Messages
        | SourceKind::Photos
        | SourceKind::Contacts
        | SourceKind::Calendar
        | SourceKind::CallLog
        | SourceKind::Clipboard
        | SourceKind::Media => {
            let t = rec.text.trim();
            if t.is_empty() {
                vec![]
            } else {
                vec![t.to_string()]
            }
        }
    };

    pieces
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let ordinal = i as u32;
            Chunk {
                chunk_id: chunk_id(rec.source, &rec.item_id, ordinal),
                source: rec.source,
                item_id: rec.item_id.clone(),
                ordinal,
                text,
                start_ms: None,
                end_ms: None,
            }
        })
        .collect()
}

/// Paragraphs separated by one or more blank lines.
fn split_paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !cur.trim().is_empty() {
                out.push(cur.trim().to_string());
            }
            cur.clear();
        } else {
            if !cur.is_empty() {
                cur.push('\n');
            }
            cur.push_str(line);
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Sections starting at Markdown-style headings (`#`). Text before the first
/// heading is its own section.
fn split_sections(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with('#') && !cur.trim().is_empty() {
            out.push(cur.trim().to_string());
            cur.clear();
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Windows of `max` tokens advancing by `max - overlap`.
fn window_text(text: &str, max: usize, overlap: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return vec![];
    }
    if words.len() <= max {
        return vec![text.trim().to_string()];
    }
    let stride = max.saturating_sub(overlap).max(1);
    let mut out = Vec::new();
    let mut start = 0;
    loop {
        let end = (start + max).min(words.len());
        out.push(words[start..end].join(" "));
        if end == words.len() {
            break;
        }
        start += stride;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rec(source: SourceKind, text: &str) -> Record {
        Record {
            source,
            item_id: "item-1".into(),
            timestamp_ms: 0,
            text: text.into(),
            metadata: BTreeMap::new(),
            uri: "x://1".into(),
        }
    }

    #[test]
    fn message_is_one_chunk() {
        let c = chunk_record(&rec(SourceKind::Messages, "wifi pw is hunter2"));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].text, "wifi pw is hunter2");
    }

    #[test]
    fn empty_text_yields_no_chunks() {
        assert!(chunk_record(&rec(SourceKind::Photos, "   ")).is_empty());
    }

    #[test]
    fn notes_split_per_paragraph() {
        let c = chunk_record(&rec(SourceKind::Notes, "first para\nline two\n\n\nsecond para"));
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].text, "first para\nline two");
        assert_eq!(c[1].text, "second para");
    }

    #[test]
    fn documents_split_at_headings() {
        let c = chunk_record(&rec(SourceKind::Files, "intro\n# A\nalpha\n# B\nbeta"));
        assert_eq!(c.len(), 3);
        assert!(c[1].text.starts_with("# A"));
        assert!(c[2].text.starts_with("# B"));
    }

    #[test]
    fn long_documents_window_with_overlap() {
        let words: Vec<String> = (0..700).map(|i| format!("w{i}")).collect();
        let c = chunk_record(&rec(SourceKind::Files, &words.join(" ")));
        // stride 255: windows start at 0, 255, 510 -> 3 chunks
        assert_eq!(c.len(), 3);
        let first: Vec<&str> = c[0].text.split_whitespace().collect();
        let second: Vec<&str> = c[1].text.split_whitespace().collect();
        assert_eq!(first.len(), DOC_WINDOW_TOKENS);
        assert_eq!(second[0], "w255");
        // last 45 tokens of window 1 are the first 45 of window 2
        assert_eq!(&first[255..], &second[..DOC_OVERLAP_TOKENS]);
        assert_eq!(c[2].text.split_whitespace().last(), Some("w699"));
    }

    #[test]
    fn chunk_ids_are_stable_and_unique() {
        let a = chunk_record(&rec(SourceKind::Notes, "p1\n\np2"));
        let b = chunk_record(&rec(SourceKind::Notes, "p1\n\np2"));
        assert_eq!(a, b);
        assert_ne!(a[0].chunk_id, a[1].chunk_id);
        assert_ne!(
            chunk_id(SourceKind::Notes, "x", 0),
            chunk_id(SourceKind::Files, "x", 0)
        );
    }
}

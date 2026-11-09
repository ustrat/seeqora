//! The normalized record every platform adapter emits.
//!
//! Adapters are read-only and thin (< 300 lines, no business logic). For the
//! same fixture data, the iOS and Android adapters must emit identical records.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Messages,
    Photos,
    Notes,
    Files,
    Contacts,
    Calendar,
    Email,
    CallLog,
    Clipboard,
    /// Video and audio (Phase 2).
    Media,
}

impl SourceKind {
    pub const ALL: [SourceKind; 10] = [
        SourceKind::Messages,
        SourceKind::Photos,
        SourceKind::Notes,
        SourceKind::Files,
        SourceKind::Contacts,
        SourceKind::Calendar,
        SourceKind::Email,
        SourceKind::CallLog,
        SourceKind::Clipboard,
        SourceKind::Media,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            SourceKind::Messages => "messages",
            SourceKind::Photos => "photos",
            SourceKind::Notes => "notes",
            SourceKind::Files => "files",
            SourceKind::Contacts => "contacts",
            SourceKind::Calendar => "calendar",
            SourceKind::Email => "email",
            SourceKind::CallLog => "call_log",
            SourceKind::Clipboard => "clipboard",
            SourceKind::Media => "media",
        }
    }

    pub fn parse(s: &str) -> Option<SourceKind> {
        SourceKind::ALL.iter().copied().find(|k| k.as_str() == s)
    }
}

/// `(source, item_id, timestamp, text, metadata, uri)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub source: SourceKind,
    /// Stable id of the item within its source (e.g. PhotoKit localIdentifier).
    pub item_id: String,
    /// Item time in Unix milliseconds.
    pub timestamp_ms: i64,
    /// Extracted text (message body, OCR + caption, note body, document text).
    pub text: String,
    /// Source-specific metadata (sender, title, album...). Sorted for stable hashing.
    pub metadata: BTreeMap<String, String>,
    /// Platform URI the shell uses for tap-through to the original (T2).
    pub uri: String,
}

impl Record {
    /// Hash of everything that affects indexing. An item is re-chunked only when
    /// this changes, so unchanged items are never re-OCR'd or re-embedded (T5).
    pub fn content_hash(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.text.as_bytes());
        h.update([0u8]);
        for (k, v) in &self.metadata {
            h.update(k.as_bytes());
            h.update([0x1f]);
            h.update(v.as_bytes());
            h.update([0x1e]);
        }
        h.update(self.timestamp_ms.to_le_bytes());
        h.update(self.uri.as_bytes());
        hex::encode(h.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(text: &str) -> Record {
        Record {
            source: SourceKind::Notes,
            item_id: "n1".into(),
            timestamp_ms: 1_700_000_000_000,
            text: text.into(),
            metadata: BTreeMap::new(),
            uri: "notes://n1".into(),
        }
    }

    #[test]
    fn hash_is_stable_and_content_sensitive() {
        assert_eq!(rec("a").content_hash(), rec("a").content_hash());
        assert_ne!(rec("a").content_hash(), rec("b").content_hash());
    }

    #[test]
    fn source_kind_round_trips() {
        for k in SourceKind::ALL {
            assert_eq!(SourceKind::parse(k.as_str()), Some(k));
        }
    }
}

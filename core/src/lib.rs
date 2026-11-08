//! Seeqora shared core.
//!
//! Everything between a source item and an answer happens here, once, for both
//! platforms (T6). Swift and Kotlin shells only read sources and render.
//!
//! Pipeline: [`record::Record`] -> [`chunk::chunk_record`] -> [`store::Store`]
//! (items, chunks, embeddings, FTS5, sources) -> [`retrieve::hybrid_search`].

pub mod chunk;
pub mod embed;
pub mod error;
pub mod record;
pub mod retrieve;
pub mod store;

pub use error::{CoreError, Result};

/// Core version; shells display it and the release tag must match it.
pub fn core_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

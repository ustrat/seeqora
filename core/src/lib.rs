//! Seeqora shared core.
//!
//! Everything between a source item and an answer happens here, once, for both
//! platforms (T6). Swift and Kotlin shells only read sources and render.
//!
//! Pipeline: [`record::Record`] -> [`chunk::chunk_record`] -> [`store::Store`]
//! (items, chunks, embeddings, FTS5, sources) -> [`retrieve::hybrid_search`]
//! -> [`generate`] (cited answer from an on-device model).

pub mod bert;
pub mod chunk;
pub mod embed;
pub mod engine;
pub mod error;
pub mod generate;
#[cfg(target_os = "android")]
pub mod llama;
pub mod qwen;
pub mod record;
pub mod retrieve;
pub mod store;
pub mod when;

pub use error::{CoreError, Result};

uniffi::setup_scaffolding!();

/// Core version; shells display it and the release tag must match it.
///
/// The FFI surface is deliberately tiny. Shells get a facade over ingestion
/// and answering, never direct access to retrieval (Licensing rules).
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

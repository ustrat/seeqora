use thiserror::Error;

/// Every failure is surfaced to the shell as a typed error so it can show a
/// plain message and keep working on what it has (T9). Never panic on user data.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// Ingestion attempted for a source the user has not opted into (T3).
    #[error("source '{0}' has not been granted by the user")]
    NoConsent(String),

    #[error("embedding failed: {0}")]
    Embedding(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

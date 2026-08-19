use thiserror::Error;

#[derive(Debug, Error)]
pub enum CfdbCliError {
    #[cfg(feature = "lang-rust")]
    #[error("extract failed: {0}")]
    Extract(#[from] cfdb_extractor::ExtractError),

    #[error("language producer failed: {0}")]
    Lang(#[from] cfdb_lang::LanguageError),

    #[error(transparent)]
    NoProducer(#[from] crate::lang::NoProducerDetected),

    #[error(transparent)]
    Store(#[from] cfdb_core::store::StoreError),

    #[error("parse error: {0}")]
    Parse(#[from] cfdb_query::parser::ParseError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Usage(String),
}

#[cfg(feature = "classify")]
impl From<cfdb_classify::ClassifyError> for CfdbCliError {
    fn from(e: cfdb_classify::ClassifyError) -> Self {
        match e {
            cfdb_classify::ClassifyError::Store(s) => CfdbCliError::Store(s),
            other => CfdbCliError::Usage(other.to_string()),
        }
    }
}

impl From<String> for CfdbCliError {
    fn from(s: String) -> Self {
        CfdbCliError::Usage(s)
    }
}

impl From<&str> for CfdbCliError {
    fn from(s: &str) -> Self {
        CfdbCliError::Usage(s.to_string())
    }
}

use std::fmt;

#[derive(Debug)]
pub enum DbError {
    ConnectionFailed(String),
    QueryFailed(String),
    DataDecodeFailed(String),
    RecordNotFound(i64),
    Other(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "Database connection failed: {msg}"),
            Self::QueryFailed(msg) => write!(f, "Database query failed: {msg}"),
            Self::DataDecodeFailed(msg) => write!(f, "Failed to decode data: {msg}"),
            Self::RecordNotFound(id) => write!(f, "No record found with id: {id}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<sqlx::Error> for DbError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => Self::Other("Row not found".to_string()),
            sqlx::Error::Database(e) => Self::QueryFailed(e.to_string()),
            sqlx::Error::PoolClosed => Self::ConnectionFailed("Pool closed".to_string()),
            sqlx::Error::PoolTimedOut => Self::ConnectionFailed("Pool timed out".to_string()),
            _ => Self::Other(error.to_string()),
        }
    }
}

impl From<serde_json::Error> for DbError {
    fn from(error: serde_json::Error) -> Self {
        Self::DataDecodeFailed(error.to_string())
    }
}

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
            DbError::ConnectionFailed(msg) => write!(f, "Database connection failed: {msg}"),
            DbError::QueryFailed(msg) => write!(f, "Database query failed: {msg}"),
            DbError::DataDecodeFailed(msg) => write!(f, "Failed to decode data: {msg}"),
            DbError::RecordNotFound(id) => write!(f, "No record found with id: {id}"),
            DbError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<sqlx::Error> for DbError {
    fn from(error: sqlx::Error) -> Self {
        match error {
            sqlx::Error::RowNotFound => DbError::Other("Row not found".to_string()),
            sqlx::Error::Database(e) => DbError::QueryFailed(e.to_string()),
            sqlx::Error::PoolClosed => DbError::ConnectionFailed("Pool closed".to_string()),
            sqlx::Error::PoolTimedOut => DbError::ConnectionFailed("Pool timed out".to_string()),
            _ => DbError::Other(error.to_string()),
        }
    }
}

impl From<serde_json::Error> for DbError {
    fn from(error: serde_json::Error) -> Self {
        DbError::DataDecodeFailed(error.to_string())
    }
}

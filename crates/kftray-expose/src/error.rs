use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum ExposeError {
    Configuration { message: String },
    KubeApi(String),
    Expose(String),
}

impl fmt::Display for ExposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration { message } => {
                write!(f, "Configuration error: {message}")
            }
            Self::KubeApi(msg) => write!(f, "Kubernetes API error: {msg}"),
            Self::Expose(msg) => write!(f, "Expose error: {msg}"),
        }
    }
}

impl std::error::Error for ExposeError {}

impl From<String> for ExposeError {
    fn from(s: String) -> Self {
        Self::Expose(s)
    }
}

pub type ExposeResult<T> = Result<T, ExposeError>;

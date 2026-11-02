use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("tool error: {0}")]
    Tool(String),

    #[error("policy denied: {0}")]
    Policy(String),

    #[error("budget exceeded: {0}")]
    Budget(String),

    #[error("cancelled")]
    Cancelled,

    #[error("maximum turns reached ({0})")]
    MaxTurns(usize),

    #[error("store error: {0}")]
    Store(String),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Cancelled => 130,
            Error::Policy(_) => 3,
            Error::Budget(_) => 4,
            Error::MaxTurns(_) => 5,
            _ => 1,
        }
    }
}

use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use crate::bot::State;

#[derive(Debug, Error)]
pub enum BotError {
    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Pool(#[from] deadpool_sqlite::PoolError),
    #[error("{0}")]
    CreatePool(#[from] deadpool_sqlite::CreatePoolError),
    #[error("{0}")]
    Interact(#[from] deadpool_sqlite::InteractError),
    #[error("{0}")]
    Url(#[from] url::ParseError),
    #[error("{0}")]
    Other(#[from] SendError<(u64, State)>),
    #[error("{0}")]
    Parse(#[from] std::num::ParseIntError),
    #[error("no env ids")]
    EnvIds,
    #[error("no completion given")]
    NoCompletionGiven,
    #[error("invalid callback query")]
    InvalidCallbackQuery,
}
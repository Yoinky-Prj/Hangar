use serde::{Serialize, Serializer};
use std::path::Path;

/// Every error crosses the IPC boundary as a plain, human-readable string.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid data file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Git: {0}")]
    Git(String),
    #[error("Network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Credential store: {0}")]
    Keyring(#[from] keyring::Error),
}

impl From<git2::Error> for AppError {
    fn from(e: git2::Error) -> Self {
        AppError::Git(e.message().to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

pub fn msg(s: impl Into<String>) -> AppError {
    AppError::Msg(s.into())
}

/// Attach the path and a friendlier explanation to an IO error.
pub fn io_ctx(e: std::io::Error, action: &str, path: &Path) -> AppError {
    let hint = match e.kind() {
        std::io::ErrorKind::PermissionDenied => {
            " A file inside it is probably open in another program (editor, terminal, dev server, Docker). Close it and try again."
        }
        std::io::ErrorKind::AlreadyExists => " Something with that name already exists.",
        std::io::ErrorKind::NotFound => " The folder no longer exists.",
        _ => "",
    };
    AppError::Msg(format!("Could not {action} {}: {e}.{hint}", path.display()))
}

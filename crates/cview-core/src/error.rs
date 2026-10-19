//! `Sources/CViewCore/Errors/AppError.swift` 포팅.

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unknown error: {0}")]
    Unknown(String),
}

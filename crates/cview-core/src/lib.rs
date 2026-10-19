//! CViewCore 포팅: 도메인 모델, 오류, 공통 유틸리티.

pub mod error;
pub mod logic;
pub mod models;
pub mod protocols;
pub mod utils;

pub use error::AppError;

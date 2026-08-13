use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, AsarError>;

#[derive(Debug, thiserror::Error)]
pub enum AsarError {
    #[error("입출력 오류 ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("입출력 오류: {0}")]
    PlainIo(#[from] std::io::Error),

    #[error("ASAR 헤더를 해석할 수 없습니다: {0}")]
    Header(String),

    #[error("ASAR 헤더 JSON 오류: {0}")]
    Json(#[from] serde_json::Error),

    #[error("허용되지 않는 경로입니다: {0}")]
    UnsafePath(String),

    #[error("아카이브에 없는 항목입니다: {0}")]
    NotFound(String),

    #[error("아카이브 항목 '{path}'의 크기가 맞지 않습니다 (기대 {expected}바이트, 실제 {actual}바이트)")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },

    #[error("아카이브 항목 '{path}'의 해시가 일치하지 않습니다")]
    IntegrityMismatch { path: String },

    #[error("지원하지 않는 항목 유형입니다: {0}")]
    UnsupportedEntry(String),
}

impl AsarError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        AsarError::Io {
            path: path.into(),
            source,
        }
    }
}

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, InstallError>;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("게임 경로를 찾을 수 없습니다. '찾아보기'로 직접 지정해주세요.")]
    GameNotFound,

    #[error("'{0}'에서 app.asar을 찾을 수 없습니다. 게임 설치 폴더가 맞는지 확인해주세요.")]
    AsarNotFound(PathBuf),

    #[error("번역 데이터 폴더를 찾을 수 없습니다: {0}")]
    DataDirNotFound(PathBuf),

    #[error("번역 데이터 폴더에 필요한 하위 폴더가 없습니다: {0}")]
    DataDirIncomplete(String),

    #[error("'{0}'에 쓰기 권한이 없습니다. 게임을 종료하고 관리자 권한으로 다시 실행해보세요.")]
    NotWritable(PathBuf),

    #[error(
        "디스크 공간이 부족합니다. 최소 {required_mb}MB가 필요하지만 {available_mb}MB만 남아 있습니다."
    )]
    NotEnoughSpace { required_mb: u64, available_mb: u64 },

    #[error("입출력 오류 ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("ASAR 처리 오류: {0}")]
    Asar(#[from] dc_asar::AsarError),

    #[error("설치 검증에 실패했습니다: {0}")]
    Verification(String),

    #[error("설치에 실패해 원본을 복구했습니다. 원인: {0}")]
    RolledBack(String),

    #[error(
        "설치에 실패했고 원본 복구도 실패했습니다. 원인: {cause} / 복구 실패: {rollback}. '{backup}' 파일을 'app.asar'로 직접 되돌려주세요."
    )]
    RollbackFailed {
        cause: String,
        rollback: String,
        backup: PathBuf,
    },

    #[error("백업 파일이 없습니다: {0}")]
    BackupMissing(PathBuf),
}

impl InstallError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        InstallError::Io {
            path: path.into(),
            source,
        }
    }
}

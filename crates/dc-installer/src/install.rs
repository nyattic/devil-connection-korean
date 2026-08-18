use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use dc_asar::{
    ArchiveRoot, AsarArchive, EntryKind, PackOptions, create_archive_observed, unpacked_dir_for,
};
use sha2::{Digest, Sha256};

use crate::cancel::Cancel;
use crate::error::{InstallError, Result};
use crate::fsutil::{self, CopyStats};
use crate::progress::{Event, Reporter, info, success, warn};

pub const PATCH_DIRS: &[&str] = &[
    "data/scenario",
    "data/others",
    "data/system",
    "data/fgimage",
    "data/image",
    "data/video",
    "data/bgimage",
    "tyrano",
];

const SPACE_MARGIN: u64 = 256 * 1024 * 1024;

const WORK_DIR_PREFIX: &str = ".dcpatch-work-";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepInfo {
    pub label: &'static str,
    pub detail: &'static str,
    pub weight: u32,
}

pub const STEPS: &[StepInfo] = &[
    StepInfo {
        label: "준비 확인",
        detail: "설치 준비 상태를 확인합니다",
        weight: 1,
    },
    StepInfo {
        label: "원본 백업",
        detail: "원본 파일을 백업합니다",
        weight: 4,
    },
    StepInfo {
        label: "원본 해제",
        detail: "원본 app.asar을 해제합니다 (시간이 걸릴 수 있습니다)",
        weight: 10,
    },
    StepInfo {
        label: "번역 적용",
        detail: "번역 데이터를 덮어씁니다",
        weight: 4,
    },
    StepInfo {
        label: "재압축",
        detail: "app.asar을 다시 만듭니다 (시간이 걸릴 수 있습니다)",
        weight: 12,
    },
    StepInfo {
        label: "검증",
        detail: "생성한 아카이브를 검증합니다",
        weight: 4,
    },
    StepInfo {
        label: "교체",
        detail: "게임 파일을 교체합니다",
        weight: 2,
    },
    StepInfo {
        label: "정리",
        detail: "임시 파일을 정리합니다",
        weight: 1,
    },
];

#[derive(Debug, Clone)]
pub enum TranslationSource {
    Directory(PathBuf),
    Embedded(&'static [u8]),
}

#[derive(Debug, Clone)]
pub struct InstallConfig {
    pub asar_path: PathBuf,
    pub source: TranslationSource,
    pub keep_work_dir: bool,
    pub cancel: Cancel,
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub asar_path: PathBuf,
    pub backup_path: PathBuf,
    pub copied_files: u64,
    pub copied_bytes: u64,
    pub archive_bytes: u64,
    pub verified_files: u64,
}

struct CopiedFile {
    archive_path: String,
    expected: [u8; 32],
}

struct Paths {
    asar: PathBuf,
    asar_unpacked: PathBuf,
    backup: PathBuf,
    backup_unpacked: PathBuf,
    work: PathBuf,
    app: PathBuf,
    new_asar: PathBuf,
    new_asar_unpacked: PathBuf,
}

impl Paths {
    fn new(asar: PathBuf, work: PathBuf) -> Self {
        let asar_unpacked = unpacked_dir_for(&asar);
        let new_asar = work.join("app.asar.new");
        let backup = with_suffix(&asar, ".backup");

        Paths {
            backup_unpacked: unpacked_dir_for(&backup),
            backup,
            asar_unpacked,
            asar,
            app: work.join("app"),
            new_asar_unpacked: unpacked_dir_for(&new_asar),
            new_asar,
            work,
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

pub fn backup_path(asar_path: &Path) -> PathBuf {
    with_suffix(asar_path, ".backup")
}

struct StepObserver<'a> {
    reporter: &'a dyn Reporter,
    cancel: &'a Cancel,
}

impl dc_asar::Observer for StepObserver<'_> {
    fn advance(&self, task: &str, done: u64, total: u64) {
        self.reporter.report(Event::Progress {
            label: task.to_string(),
            done,
            total,
        });
    }

    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

fn step(reporter: &dyn Reporter, index: u32) {
    reporter.report(Event::Step {
        index,
        total: STEPS.len() as u32,
        message: STEPS[index as usize - 1].detail.to_string(),
    });
}

pub fn install(config: &InstallConfig, reporter: &dyn Reporter) -> Result<InstallReport> {
    step(reporter, 1);
    let preflight = preflight(config, reporter)?;

    let work = fsutil::create_work_dir(&preflight.resources, WORK_DIR_PREFIX)?;
    let paths = Paths::new(config.asar_path.clone(), work);

    let outcome = run_install(config, &paths, &preflight, reporter);

    if config.keep_work_dir {
        info(
            reporter,
            format!("작업 폴더를 남깁니다: {}", paths.work.display()),
        );
    } else if let Err(e) = fsutil::remove_path(&paths.work) {
        warn(reporter, format!("작업 폴더 정리에 실패했습니다: {e}"));
    }

    outcome
}

fn run_install(
    config: &InstallConfig,
    paths: &Paths,
    preflight: &Preflight,
    reporter: &dyn Reporter,
) -> Result<InstallReport> {
    let observer = StepObserver {
        reporter,
        cancel: &config.cancel,
    };

    config.cancel.check()?;
    step(reporter, 2);
    ensure_backup(paths, reporter)?;

    config.cancel.check()?;
    step(reporter, 3);
    let mut source_archive = AsarArchive::open(&paths.backup)?;
    source_archive.validate()?;
    let source_entries = source_archive.entries();
    let original_entries = source_entries.len();
    let preserve_unpacked: BTreeSet<String> = source_entries
        .into_iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File { unpacked: true, .. }))
        .map(|entry| entry.path)
        .collect();
    let extract_stats = source_archive.extract_to_observed(&paths.app, &observer)?;
    drop(source_archive);
    success(
        reporter,
        format!(
            "해제 완료: 파일 {}개, 폴더 {}개",
            extract_stats.files, extract_stats.directories
        ),
    );
    if !preserve_unpacked.is_empty() {
        info(
            reporter,
            format!(
                "원본이 unpacked로 둔 파일 {}개를 그대로 유지합니다",
                preserve_unpacked.len()
            ),
        );
    }

    config.cancel.check()?;
    step(reporter, 4);
    let (copy_stats, copied) = apply_translation(
        &config.source,
        &paths.app,
        reporter,
        &config.cancel,
        &observer,
    )?;
    success(
        reporter,
        format!(
            "번역 파일 {}개 적용 ({}MB)",
            copy_stats.files,
            fsutil::to_mb(copy_stats.bytes)
        ),
    );

    config.cancel.check()?;
    step(reporter, 5);
    let options = PackOptions {
        unpack: vec!["*.node".to_string()],
        preserve_unpacked,
    };
    let roots = [ArchiveRoot {
        archive_path: "",
        source: &paths.app,
    }];
    let pack_stats = create_archive_observed(&roots, &paths.new_asar, &options, &observer)?;
    success(
        reporter,
        format!(
            "생성 완료: {}MB, 파일 {}개 (unpacked {}개)",
            fsutil::to_mb(pack_stats.archive_bytes),
            pack_stats.files,
            pack_stats.unpacked_files
        ),
    );

    config.cancel.check()?;
    step(reporter, 6);
    let verified = verify(
        &paths.new_asar,
        &copied,
        original_entries,
        reporter,
        &config.cancel,
    )?;

    config.cancel.check()?;
    step(reporter, 7);
    if let Err(cause) = commit(paths) {
        warn(reporter, "교체에 실패했습니다. 원본을 복구합니다.");
        return Err(match rollback(paths) {
            Ok(()) => InstallError::RolledBack(cause.to_string()),
            Err(rollback_err) => InstallError::RollbackFailed {
                cause: cause.to_string(),
                rollback: rollback_err.to_string(),
                backup: paths.backup.clone(),
            },
        });
    }
    success(reporter, "교체 완료");

    step(reporter, 8);
    let stale_app = preflight.resources.join("app");
    if stale_app.is_dir() {
        info(reporter, "이전 설치가 남긴 app 폴더를 삭제합니다");
        fsutil::remove_path(&stale_app)?;
    }

    Ok(InstallReport {
        asar_path: paths.asar.clone(),
        backup_path: paths.backup.clone(),
        copied_files: copy_stats.files,
        copied_bytes: copy_stats.bytes,
        archive_bytes: pack_stats.archive_bytes,
        verified_files: verified,
    })
}

struct Preflight {
    resources: PathBuf,
}

fn preflight(config: &InstallConfig, reporter: &dyn Reporter) -> Result<Preflight> {
    let asar = &config.asar_path;
    if !asar.is_file() {
        return Err(InstallError::AsarNotFound(asar.to_path_buf()));
    }
    let resources = asar
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    info(reporter, format!("대상: {}", asar.display()));

    let data_size = check_source(&config.source)?;
    fsutil::check_writable(&resources)?;

    let archive = AsarArchive::open(asar)?;
    archive.validate()?;
    drop(archive);

    let archive_size = fs::metadata(asar)
        .map_err(|e| InstallError::io(asar, e))?
        .len();

    let backup_exists = backup_path(asar).is_file();
    let required = archive_size * if backup_exists { 2 } else { 3 } + data_size * 2 + SPACE_MARGIN;
    let available = fsutil::available_space(&resources)?;
    if available < required {
        return Err(InstallError::NotEnoughSpace {
            required_mb: fsutil::to_mb(required),
            available_mb: fsutil::to_mb(available),
        });
    }

    info(
        reporter,
        format!(
            "원본 {}MB, 번역 데이터 {}MB, 여유 공간 {}MB",
            fsutil::to_mb(archive_size),
            fsutil::to_mb(data_size),
            fsutil::to_mb(available)
        ),
    );

    Ok(Preflight { resources })
}

pub fn find_data_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.extend(dir.ancestors().take(5).map(Path::to_path_buf));
        candidates.push(dir.join("../Resources"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.extend(cwd.ancestors().take(3).map(Path::to_path_buf));
    }

    candidates
        .into_iter()
        .find(|candidate| check_data_dir(candidate).is_ok())
}

fn check_data_dir(data_dir: &Path) -> Result<()> {
    if !data_dir.is_dir() {
        return Err(InstallError::DataDirNotFound(data_dir.to_path_buf()));
    }

    let missing: Vec<&str> = PATCH_DIRS
        .iter()
        .copied()
        .filter(|dir| !data_dir.join(dir).is_dir())
        .collect();

    if !missing.is_empty() {
        return Err(InstallError::DataDirIncomplete(missing.join(", ")));
    }

    Ok(())
}

fn check_source(source: &TranslationSource) -> Result<u64> {
    match source {
        TranslationSource::Directory(dir) => {
            check_data_dir(dir)?;
            let mut total = 0u64;
            for patch_dir in PATCH_DIRS {
                total += fsutil::dir_size(&dir.join(patch_dir))?;
            }
            Ok(total)
        }
        TranslationSource::Embedded(bytes) => {
            let archive = AsarArchive::from_bytes(bytes)?;
            archive.validate()?;

            let directories: Vec<String> = archive
                .entries()
                .into_iter()
                .filter(|entry| matches!(entry.kind, dc_asar::EntryKind::Directory))
                .map(|entry| entry.path)
                .collect();
            let missing: Vec<&str> = PATCH_DIRS
                .iter()
                .copied()
                .filter(|dir| !directories.iter().any(|found| found == dir))
                .collect();

            if !missing.is_empty() {
                return Err(InstallError::DataDirIncomplete(missing.join(", ")));
            }

            Ok(bytes.len() as u64)
        }
    }
}

fn ensure_backup(paths: &Paths, reporter: &dyn Reporter) -> Result<()> {
    if paths.backup.is_file() {
        info(reporter, "기존 백업을 그대로 사용합니다");
    } else {
        let staging = with_suffix(&paths.asar, ".backup.partial");
        fsutil::remove_path(&staging)?;
        fsutil::copy_durable(&paths.asar, &staging)?;
        fsutil::move_replace(&staging, &paths.backup)?;
        success(reporter, format!("백업 생성: {}", paths.backup.display()));
    }

    if paths.asar_unpacked.is_dir() && !paths.backup_unpacked.exists() {
        fsutil::copy_dir_all(&paths.asar_unpacked, &paths.backup_unpacked)?;
        success(
            reporter,
            format!("unpacked 백업 생성: {}", paths.backup_unpacked.display()),
        );
    }

    Ok(())
}

fn apply_translation(
    source: &TranslationSource,
    app_dir: &Path,
    reporter: &dyn Reporter,
    cancel: &Cancel,
    observer: &StepObserver,
) -> Result<(CopyStats, Vec<CopiedFile>)> {
    match source {
        TranslationSource::Directory(dir) => apply_from_dir(dir, app_dir, reporter, cancel),
        TranslationSource::Embedded(bytes) => {
            apply_from_archive(bytes, app_dir, reporter, cancel, observer)
        }
    }
}

fn apply_from_archive(
    bytes: &[u8],
    app_dir: &Path,
    reporter: &dyn Reporter,
    cancel: &Cancel,
    observer: &StepObserver,
) -> Result<(CopyStats, Vec<CopiedFile>)> {
    let mut archive = AsarArchive::from_bytes(bytes)?;
    archive.validate()?;
    let extracted = archive.extract_to_observed(app_dir, observer)?;

    let paths: Vec<String> = archive
        .entries()
        .into_iter()
        .filter(|entry| matches!(entry.kind, EntryKind::File { .. }))
        .map(|entry| entry.path)
        .collect();

    let total = paths.len() as u64;
    let mut copied = Vec::with_capacity(paths.len());
    for (index, archive_path) in paths.into_iter().enumerate() {
        cancel.check()?;
        let expected = archive.hash_file(&archive_path)?;
        copied.push(CopiedFile {
            archive_path,
            expected,
        });

        if index % 100 == 0 || index as u64 + 1 == total {
            reporter.report(Event::Progress {
                label: "번역 데이터 확인".to_string(),
                done: index as u64 + 1,
                total,
            });
        }
    }

    let stats = CopyStats {
        files: extracted.files,
        bytes: extracted.bytes,
    };
    Ok((stats, copied))
}

fn apply_from_dir(
    data_dir: &Path,
    app_dir: &Path,
    reporter: &dyn Reporter,
    cancel: &Cancel,
) -> Result<(CopyStats, Vec<CopiedFile>)> {
    let mut stats = CopyStats::default();
    let mut copied = Vec::new();

    for (index, dir) in PATCH_DIRS.iter().enumerate() {
        cancel.check()?;
        let src = data_dir.join(dir);
        let dst = app_dir.join(dir);

        reporter.report(Event::Progress {
            label: (*dir).to_string(),
            done: index as u64 + 1,
            total: PATCH_DIRS.len() as u64,
        });

        let sub = fsutil::copy_dir_all(&src, &dst)?;
        stats.files += sub.files;
        stats.bytes += sub.bytes;
        collect_files(&src, dir, &mut copied)?;
    }

    Ok((stats, copied))
}

fn collect_files(src: &Path, prefix: &str, out: &mut Vec<CopiedFile>) -> Result<()> {
    for item in fs::read_dir(src).map_err(|e| InstallError::io(src, e))? {
        let item = item.map_err(|e| InstallError::io(src, e))?;
        let path = item.path();
        let Some(name) = item.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let rel = format!("{prefix}/{name}");

        let metadata = fs::symlink_metadata(&path).map_err(|e| InstallError::io(&path, e))?;
        if metadata.is_dir() {
            collect_files(&path, &rel, out)?;
        } else if metadata.is_file() {
            out.push(CopiedFile {
                archive_path: rel,
                expected: hash_file(&path)?,
            });
        }
    }

    Ok(())
}

fn verify(
    new_asar: &Path,
    copied: &[CopiedFile],
    original_entries: usize,
    reporter: &dyn Reporter,
    cancel: &Cancel,
) -> Result<u64> {
    let mut archive = AsarArchive::open(new_asar)?;
    archive.validate()?;

    let entries = archive.entries().len();
    if entries < original_entries {
        return Err(InstallError::Verification(format!(
            "항목 수가 원본보다 적습니다 (원본 {original_entries}개, 생성본 {entries}개)"
        )));
    }

    let total = copied.len() as u64;
    for (index, file) in copied.iter().enumerate() {
        cancel.check()?;
        let actual = archive.hash_file(&file.archive_path).map_err(|e| {
            InstallError::Verification(format!("'{}' 확인 실패: {e}", file.archive_path))
        })?;

        if file.expected != actual {
            return Err(InstallError::Verification(format!(
                "'{}'의 내용이 원본 번역 파일과 다릅니다",
                file.archive_path
            )));
        }

        if index % 100 == 0 || index as u64 + 1 == total {
            reporter.report(Event::Progress {
                label: "번역 파일 검증".to_string(),
                done: index as u64 + 1,
                total,
            });
        }
    }

    success(reporter, format!("번역 파일 {total}개 검증 완료"));
    Ok(total)
}

fn hash_file(path: &Path) -> Result<[u8; 32]> {
    use std::io::Read;

    let mut file = fs::File::open(path).map_err(|e| InstallError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];

    loop {
        let read = file.read(&mut buf).map_err(|e| InstallError::io(path, e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    Ok(hasher.finalize().into())
}

fn commit(paths: &Paths) -> Result<()> {
    if paths.asar_unpacked.is_dir() {
        fsutil::move_replace(&paths.asar_unpacked, &paths.work.join("prev.unpacked"))?;
    }
    if paths.new_asar_unpacked.is_dir() {
        fsutil::move_replace(&paths.new_asar_unpacked, &paths.asar_unpacked)?;
    }
    fsutil::move_replace(&paths.new_asar, &paths.asar)?;
    Ok(())
}

fn rollback(paths: &Paths) -> Result<()> {
    if !paths.backup.is_file() {
        return Err(InstallError::BackupMissing(paths.backup.clone()));
    }

    fsutil::remove_path(&paths.asar)?;
    fsutil::copy_durable(&paths.backup, &paths.asar)?;

    fsutil::remove_path(&paths.asar_unpacked)?;
    if paths.backup_unpacked.is_dir() {
        fsutil::copy_dir_all(&paths.backup_unpacked, &paths.asar_unpacked)?;
    }

    Ok(())
}

pub fn restore(asar_path: &Path, reporter: &dyn Reporter) -> Result<()> {
    let backup = backup_path(asar_path);
    if !backup.is_file() {
        return Err(InstallError::BackupMissing(backup));
    }

    let resources = asar_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fsutil::check_writable(&resources)?;

    AsarArchive::open(&backup)?.validate()?;

    let asar_unpacked = unpacked_dir_for(asar_path);
    let backup_unpacked = unpacked_dir_for(&backup);

    fsutil::remove_path(asar_path)?;
    fsutil::copy_durable(&backup, asar_path)?;

    fsutil::remove_path(&asar_unpacked)?;
    if backup_unpacked.is_dir() {
        fsutil::copy_dir_all(&backup_unpacked, &asar_unpacked)?;
    }

    success(
        reporter,
        format!("원본을 복구했습니다: {}", asar_path.display()),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_suffix_appends_to_full_name() {
        assert_eq!(
            with_suffix(Path::new("/games/resources/app.asar"), ".backup"),
            PathBuf::from("/games/resources/app.asar.backup")
        );
    }

    #[test]
    fn check_data_dir_reports_every_missing_folder() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("data/scenario")).unwrap();

        let err = check_data_dir(tmp.path()).unwrap_err();
        let InstallError::DataDirIncomplete(missing) = err else {
            panic!("예상과 다른 오류: {err}");
        };
        assert!(missing.contains("tyrano"));
        assert!(missing.contains("data/image"));
        assert!(!missing.contains("data/scenario"));
    }

    #[test]
    fn check_data_dir_rejects_missing_root() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            check_data_dir(&tmp.path().join("nope")),
            Err(InstallError::DataDirNotFound(_))
        ));
    }
}

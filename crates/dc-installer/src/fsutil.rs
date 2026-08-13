use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{InstallError, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyStats {
    pub files: u64,
    pub bytes: u64,
}

pub fn copy_dir_all(src: &Path, dst: &Path) -> Result<CopyStats> {
    let mut stats = CopyStats::default();
    fs::create_dir_all(dst).map_err(|e| InstallError::io(dst, e))?;

    for item in fs::read_dir(src).map_err(|e| InstallError::io(src, e))? {
        let item = item.map_err(|e| InstallError::io(src, e))?;
        let from = item.path();
        let to = dst.join(item.file_name());
        let metadata = fs::symlink_metadata(&from).map_err(|e| InstallError::io(&from, e))?;

        if metadata.is_dir() {
            let sub = copy_dir_all(&from, &to)?;
            stats.files += sub.files;
            stats.bytes += sub.bytes;
        } else {
            if to.symlink_metadata().is_ok() {
                fs::remove_file(&to).map_err(|e| InstallError::io(&to, e))?;
            }
            let bytes = fs::copy(&from, &to).map_err(|e| InstallError::io(&from, e))?;
            stats.files += 1;
            stats.bytes += bytes;
        }
    }

    Ok(stats)
}

pub fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    if !path.exists() {
        return Ok(0);
    }

    for item in fs::read_dir(path).map_err(|e| InstallError::io(path, e))? {
        let item = item.map_err(|e| InstallError::io(path, e))?;
        let child = item.path();
        let metadata = fs::symlink_metadata(&child).map_err(|e| InstallError::io(&child, e))?;
        if metadata.is_dir() {
            total += dir_size(&child)?;
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }

    Ok(total)
}

pub fn remove_path(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };

    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|e| InstallError::io(path, e))
    } else {
        fs::remove_file(path).map_err(|e| InstallError::io(path, e))
    }
}

pub fn move_replace(src: &Path, dst: &Path) -> Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) if dst.symlink_metadata().is_ok() => {
            remove_path(dst)?;
            fs::rename(src, dst).map_err(|e| InstallError::io(dst, e))
        }
        Err(e) => Err(InstallError::io(dst, e)),
    }
}

pub fn check_writable(dir: &Path) -> Result<()> {
    let probe = dir.join(".dcpatch-write-test");
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(InstallError::NotWritable(dir.to_path_buf())),
    }
}

pub fn available_space(path: &Path) -> Result<u64> {
    let mut probe: &Path = path;
    loop {
        if probe.exists() {
            return fs4::available_space(probe).map_err(|e| InstallError::io(probe, e));
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return Err(InstallError::io(path, std::io::ErrorKind::NotFound.into())),
        }
    }
}

pub fn create_work_dir(parent: &Path, prefix: &str) -> Result<PathBuf> {
    for attempt in 0..1000u32 {
        let candidate = parent.join(format!("{prefix}{attempt}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(InstallError::io(&candidate, e)),
        }
    }
    Err(InstallError::io(
        parent,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "작업 디렉터리 이름을 확보하지 못했습니다",
        ),
    ))
}

pub fn copy_durable(src: &Path, dst: &Path) -> Result<u64> {
    let bytes = fs::copy(src, dst).map_err(|e| InstallError::io(src, e))?;
    let file = fs::OpenOptions::new()
        .write(true)
        .open(dst)
        .map_err(|e| InstallError::io(dst, e))?;
    file.sync_all().map_err(|e| InstallError::io(dst, e))?;
    Ok(bytes)
}

pub fn to_mb(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_dir_all_merges_into_existing_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        fs::create_dir_all(src.join("a")).unwrap();
        fs::write(src.join("a/new.txt"), "새 파일").unwrap();
        fs::write(src.join("a/over.txt"), "덮어쓴 값").unwrap();

        fs::create_dir_all(dst.join("a")).unwrap();
        fs::write(dst.join("a/over.txt"), "원래 값").unwrap();
        fs::write(dst.join("a/keep.txt"), "유지").unwrap();

        let stats = copy_dir_all(&src, &dst).unwrap();
        assert_eq!(stats.files, 2);
        assert_eq!(
            fs::read_to_string(dst.join("a/over.txt")).unwrap(),
            "덮어쓴 값"
        );
        assert_eq!(fs::read_to_string(dst.join("a/keep.txt")).unwrap(), "유지");
        assert_eq!(
            fs::read_to_string(dst.join("a/new.txt")).unwrap(),
            "새 파일"
        );
    }

    #[test]
    fn copy_durable_copies_and_survives_an_existing_target() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.bin");
        let dst = tmp.path().join("dst.bin");
        fs::write(&src, b"payload").unwrap();
        fs::write(&dst, b"stale").unwrap();

        let bytes = copy_durable(&src, &dst).unwrap();

        assert_eq!(bytes, 7);
        assert_eq!(fs::read(&dst).unwrap(), b"payload");
    }

    #[test]
    fn dir_size_sums_nested_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        fs::write(tmp.path().join("a/x"), vec![0u8; 100]).unwrap();
        fs::write(tmp.path().join("a/b/y"), vec![0u8; 250]).unwrap();
        assert_eq!(dir_size(tmp.path()).unwrap(), 350);
        assert_eq!(dir_size(&tmp.path().join("missing")).unwrap(), 0);
    }

    #[test]
    fn move_replace_overwrites_existing_target() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.txt");
        let dst = tmp.path().join("dst.txt");
        fs::write(&src, "new").unwrap();
        fs::write(&dst, "old").unwrap();

        move_replace(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(&dst).unwrap(), "new");
        assert!(!src.exists());
    }

    #[test]
    fn create_work_dir_avoids_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        let a = create_work_dir(tmp.path(), ".work-").unwrap();
        let b = create_work_dir(tmp.path(), ".work-").unwrap();
        assert_ne!(a, b);
        assert!(a.is_dir() && b.is_dir());
    }

    #[test]
    fn available_space_walks_up_to_existing_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does/not/exist");
        assert!(available_space(&missing).unwrap() > 0);
    }
}

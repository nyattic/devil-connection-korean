use std::path::{Component, Path, PathBuf};

use crate::error::{AsarError, Result};

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn check_component(name: &str) -> Result<()> {
    let reject = |reason: &str| Err(AsarError::UnsafePath(format!("{name} ({reason})")));

    if name.is_empty() {
        return reject("빈 이름");
    }
    if name == "." || name == ".." {
        return reject("상위 경로 참조");
    }
    if name.contains('/') || name.contains('\\') {
        return reject("경로 구분자 포함");
    }
    if name.contains('\0') {
        return reject("NUL 문자 포함");
    }
    if name.contains(':') {
        return reject("드라이브 또는 스트림 구분자 포함");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return reject("Windows에서 해석이 달라지는 후행 문자");
    }

    let stem = name.split('.').next().unwrap_or(name);
    if WINDOWS_RESERVED
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return reject("Windows 예약 장치 이름");
    }

    Ok(())
}

pub fn check_rel_path(rel_path: &str) -> Result<()> {
    if rel_path.is_empty() {
        return Err(AsarError::UnsafePath("빈 경로".to_string()));
    }
    for segment in rel_path.split('/') {
        check_component(segment)?;
    }
    Ok(())
}

pub fn join_checked(base: &Path, rel_path: &str) -> Result<PathBuf> {
    check_rel_path(rel_path)?;
    let mut out = base.to_path_buf();
    for segment in rel_path.split('/') {
        out.push(segment);
    }
    Ok(out)
}

pub fn check_link_target(link_dir: &str, target: &str) -> Result<()> {
    if target.contains('\0') {
        return Err(AsarError::UnsafePath(format!("{target} (NUL 문자 포함)")));
    }

    let target_path = Path::new(target);
    if target_path.is_absolute() || target.starts_with('/') || target.starts_with('\\') {
        return Err(AsarError::UnsafePath(format!("{target} (절대 경로 링크)")));
    }

    let mut depth: i64 = link_dir.split('/').filter(|s| !s.is_empty()).count() as i64;
    for component in target_path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(AsarError::UnsafePath(format!(
                        "{target} (아카이브 루트 밖을 가리키는 링크)"
                    )));
                }
            }
            Component::Normal(_) => depth += 1,
            Component::RootDir | Component::Prefix(_) => {
                return Err(AsarError::UnsafePath(format!("{target} (절대 경로 링크)")));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_names() {
        check_component("first.ks").unwrap();
        check_component("でびるコネクショん").unwrap();
        check_rel_path("data/scenario/first.ks").unwrap();
    }

    #[test]
    fn rejects_traversal_and_separators() {
        assert!(check_component("..").is_err());
        assert!(check_component(".").is_err());
        assert!(check_component("a/b").is_err());
        assert!(check_component("a\\b").is_err());
        assert!(check_rel_path("data/../../etc/passwd").is_err());
        assert!(check_rel_path("").is_err());
        assert!(check_rel_path("data//first.ks").is_err());
    }

    #[test]
    fn rejects_windows_hostile_names() {
        assert!(check_component("C:file").is_err());
        assert!(check_component("file.txt.").is_err());
        assert!(check_component("file.txt ").is_err());
        assert!(check_component("NUL").is_err());
        assert!(check_component("con.txt").is_err());
        assert!(check_component("console.txt").is_ok());
    }

    #[test]
    fn join_checked_stays_under_base() {
        let base = Path::new("/tmp/app");
        assert_eq!(
            join_checked(base, "data/first.ks").unwrap(),
            Path::new("/tmp/app/data/first.ks")
        );
        assert!(join_checked(base, "../escape").is_err());
    }

    #[test]
    fn link_targets_must_stay_inside_archive() {
        check_link_target("data/scenario", "../others/x.js").unwrap();
        check_link_target("", "sibling.js").unwrap();
        assert!(check_link_target("", "../outside.js").is_err());
        assert!(check_link_target("data", "../../outside.js").is_err());
        assert!(check_link_target("data", "/etc/passwd").is_err());
    }
}

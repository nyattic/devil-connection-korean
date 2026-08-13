use std::fs;
use std::path::{Path, PathBuf};

use dc_asar::{ArchiveRoot, AsarArchive, PackOptions, create_archive, create_archive_from};
use dc_installer::{
    InstallConfig, InstallError, PATCH_DIRS, SilentReporter, TranslationSource, install, restore,
};

struct Fixture {
    _tmp: tempfile::TempDir,
    game_dir: PathBuf,
    asar: PathBuf,
    data_dir: PathBuf,
    pristine: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let original = root.join("original-app");
        write_file(&original.join("index.html"), b"<html>original</html>");
        write_file(&original.join("package.json"), br#"{"main":"index.js"}"#);
        write_file(
            &original.join("data/scenario/first.ks"),
            "원본 시나리오".as_bytes(),
        );
        write_file(
            &original.join("data/scenario/keep.ks"),
            "건드리지 않음".as_bytes(),
        );
        write_file(&original.join("data/image/logo.png"), b"\x89PNG original");
        write_file(&original.join("data/others/master_data.js"), b"var m={};");
        write_file(&original.join("tyrano/tyrano.css"), b"body{color:red}");
        write_file(&original.join("bin/native.node"), b"native module");

        let game_dir = root.join("game");
        let asar = game_dir.join("resources/app.asar");
        fs::create_dir_all(asar.parent().unwrap()).unwrap();
        create_archive(&original, &asar, &PackOptions::default()).unwrap();
        let pristine = fs::read(&asar).unwrap();

        let data_dir = root.join("patch-data");
        for dir in PATCH_DIRS {
            fs::create_dir_all(data_dir.join(dir)).unwrap();
        }
        write_file(
            &data_dir.join("data/scenario/first.ks"),
            "번역된 시나리오".as_bytes(),
        );
        write_file(
            &data_dir.join("data/scenario/added.ks"),
            "새로 추가된 파일".as_bytes(),
        );
        write_file(&data_dir.join("data/image/logo.png"), b"\x89PNG translated");
        write_file(&data_dir.join("tyrano/tyrano.css"), b"body{color:blue}");

        Fixture {
            _tmp: tmp,
            game_dir,
            asar,
            data_dir,
            pristine,
        }
    }

    fn config(&self) -> InstallConfig {
        InstallConfig {
            asar_path: self.asar.clone(),
            source: TranslationSource::Directory(self.data_dir.clone()),
            integrity: false,
            keep_work_dir: false,
        }
    }

    fn embedded_config(&self, roots: &[&str]) -> InstallConfig {
        let packed = self._tmp.path().join("translation.asar");
        let sources: Vec<PathBuf> = roots.iter().map(|name| self.data_dir.join(name)).collect();
        let entries: Vec<ArchiveRoot> = roots
            .iter()
            .zip(&sources)
            .map(|(name, source)| ArchiveRoot {
                archive_path: name,
                source,
            })
            .collect();

        create_archive_from(
            &entries,
            &packed,
            &PackOptions {
                unpack: Vec::new(),
                integrity: false,
            },
        )
        .unwrap();

        let bytes: &'static [u8] = Box::leak(fs::read(&packed).unwrap().into_boxed_slice());
        InstallConfig {
            asar_path: self.asar.clone(),
            source: TranslationSource::Embedded(bytes),
            integrity: false,
            keep_work_dir: false,
        }
    }

    fn backup(&self) -> PathBuf {
        self.asar.with_file_name("app.asar.backup")
    }

    fn resources(&self) -> PathBuf {
        self.game_dir.join("resources")
    }

    fn read_from_archive(&self, rel: &str) -> Vec<u8> {
        AsarArchive::open(&self.asar)
            .unwrap()
            .read_file(rel)
            .unwrap()
    }
}

fn write_file(path: &Path, contents: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn assert_no_work_dirs(resources: &Path) {
    let leftovers: Vec<String> = fs::read_dir(resources)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".dcpatch-work-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "작업 폴더가 남았습니다: {leftovers:?}"
    );
}

#[test]
fn install_applies_translation_and_keeps_untouched_files() {
    let fx = Fixture::new();
    let report = install(&fx.config(), &SilentReporter).unwrap();

    assert_eq!(
        fx.read_from_archive("data/scenario/first.ks"),
        "번역된 시나리오".as_bytes()
    );
    assert_eq!(
        fx.read_from_archive("data/scenario/added.ks"),
        "새로 추가된 파일".as_bytes()
    );
    assert_eq!(
        fx.read_from_archive("data/image/logo.png"),
        b"\x89PNG translated"
    );
    assert_eq!(
        fx.read_from_archive("tyrano/tyrano.css"),
        b"body{color:blue}"
    );

    assert_eq!(
        fx.read_from_archive("data/scenario/keep.ks"),
        "건드리지 않음".as_bytes()
    );
    assert_eq!(fx.read_from_archive("index.html"), b"<html>original</html>");
    assert_eq!(
        fx.read_from_archive("data/others/master_data.js"),
        b"var m={};"
    );

    assert!(
        fx.asar
            .with_file_name("app.asar.unpacked")
            .join("bin/native.node")
            .is_file()
    );

    assert_eq!(report.verified_files, 4);
    assert!(fx.backup().is_file());
    assert_eq!(fs::read(fx.backup()).unwrap(), fx.pristine);
    assert_no_work_dirs(&fx.resources());
}

#[test]
fn reinstall_is_idempotent_and_never_overwrites_the_backup() {
    let fx = Fixture::new();
    install(&fx.config(), &SilentReporter).unwrap();
    let after_first = fs::read(&fx.asar).unwrap();

    install(&fx.config(), &SilentReporter).unwrap();
    let after_second = fs::read(&fx.asar).unwrap();

    assert_eq!(after_first, after_second, "재설치 결과가 달라졌습니다");
    assert_eq!(
        fs::read(fx.backup()).unwrap(),
        fx.pristine,
        "백업이 패치본으로 덮어써졌습니다"
    );
    assert_no_work_dirs(&fx.resources());
}

#[test]
fn restore_returns_the_original_archive() {
    let fx = Fixture::new();
    install(&fx.config(), &SilentReporter).unwrap();
    assert_ne!(fs::read(&fx.asar).unwrap(), fx.pristine);

    restore(&fx.asar, &SilentReporter).unwrap();

    assert_eq!(fs::read(&fx.asar).unwrap(), fx.pristine);
    assert_eq!(
        fx.read_from_archive("data/scenario/first.ks"),
        "원본 시나리오".as_bytes()
    );
}

#[test]
fn restore_without_backup_reports_clearly() {
    let fx = Fixture::new();
    let err = restore(&fx.asar, &SilentReporter).unwrap_err();
    assert!(
        matches!(err, InstallError::BackupMissing(_)),
        "예상과 다른 오류: {err}"
    );
    assert_eq!(fs::read(&fx.asar).unwrap(), fx.pristine);
}

#[test]
fn incomplete_data_dir_fails_before_touching_the_game() {
    let fx = Fixture::new();
    fs::remove_dir_all(fx.data_dir.join("data/video")).unwrap();

    let err = install(&fx.config(), &SilentReporter).unwrap_err();
    let InstallError::DataDirIncomplete(missing) = &err else {
        panic!("예상과 다른 오류: {err}");
    };
    assert!(missing.contains("data/video"));

    assert_eq!(fs::read(&fx.asar).unwrap(), fx.pristine);
    assert!(!fx.backup().exists());
    assert_no_work_dirs(&fx.resources());
}

#[test]
fn corrupted_target_archive_fails_before_touching_the_game() {
    let fx = Fixture::new();
    fs::write(&fx.asar, "이것은 ASAR 아카이브가 아닙니다".as_bytes()).unwrap();

    let err = install(&fx.config(), &SilentReporter).unwrap_err();
    assert!(
        matches!(err, InstallError::Asar(_)),
        "예상과 다른 오류: {err}"
    );
    assert!(!fx.backup().exists());
    assert_no_work_dirs(&fx.resources());
}

#[test]
fn missing_asar_reports_clearly() {
    let fx = Fixture::new();
    fs::remove_file(&fx.asar).unwrap();

    let err = install(&fx.config(), &SilentReporter).unwrap_err();
    assert!(
        matches!(err, InstallError::AsarNotFound(_)),
        "예상과 다른 오류: {err}"
    );
}

#[test]
fn stale_app_folder_from_the_python_installer_is_removed() {
    let fx = Fixture::new();
    let stale = fx.resources().join("app");
    write_file(&stale.join("index.html"), b"stale");

    install(&fx.config(), &SilentReporter).unwrap();

    assert!(!stale.exists(), "resources/app 폴더가 남았습니다");
}

#[test]
fn install_recovers_when_a_previous_run_was_interrupted() {
    let fx = Fixture::new();
    let orphan_work = fx.resources().join(".dcpatch-work-0");
    write_file(&orphan_work.join("app/index.html"), b"leftover");
    fs::write(
        fx.asar.with_file_name("app.asar.backup.partial"),
        "잘린 백업".as_bytes(),
    )
    .unwrap();

    install(&fx.config(), &SilentReporter).unwrap();

    assert_eq!(
        fx.read_from_archive("data/scenario/first.ks"),
        "번역된 시나리오".as_bytes()
    );
    assert_eq!(fs::read(fx.backup()).unwrap(), fx.pristine);
}

#[test]
fn keep_work_dir_leaves_the_working_copy_behind() {
    let fx = Fixture::new();
    let config = InstallConfig {
        keep_work_dir: true,
        ..fx.config()
    };
    install(&config, &SilentReporter).unwrap();

    let work_dirs: Vec<PathBuf> = fs::read_dir(fx.resources())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(".dcpatch-work-"))
        })
        .collect();

    assert_eq!(work_dirs.len(), 1, "작업 폴더가 남아야 합니다");
    assert!(work_dirs[0].join("app/index.html").is_file());
}

#[test]
fn embedded_archive_produces_the_same_result_as_a_data_folder() {
    let expected = {
        let fx = Fixture::new();
        install(&fx.config(), &SilentReporter).unwrap();
        fs::read(&fx.asar).unwrap()
    };

    let fx = Fixture::new();
    let report = install(&fx.embedded_config(&["data", "tyrano"]), &SilentReporter).unwrap();

    assert_eq!(
        fs::read(&fx.asar).unwrap(),
        expected,
        "임베드 설치 결과가 폴더 설치와 다릅니다"
    );
    assert_eq!(report.verified_files, 4);
    assert_eq!(
        fx.read_from_archive("data/scenario/first.ks"),
        "번역된 시나리오".as_bytes()
    );
    assert_eq!(
        fx.read_from_archive("data/scenario/keep.ks"),
        "건드리지 않음".as_bytes()
    );
    assert_eq!(fx.read_from_archive("index.html"), b"<html>original</html>");
    assert_eq!(fs::read(fx.backup()).unwrap(), fx.pristine);
    assert_no_work_dirs(&fx.resources());
}

#[test]
fn incomplete_embedded_archive_fails_before_touching_the_game() {
    let fx = Fixture::new();

    let err = install(&fx.embedded_config(&["data"]), &SilentReporter).unwrap_err();
    let InstallError::DataDirIncomplete(missing) = &err else {
        panic!("예상과 다른 오류: {err}");
    };
    assert!(missing.contains("tyrano"));

    assert_eq!(fs::read(&fx.asar).unwrap(), fx.pristine);
    assert!(!fx.backup().exists());
    assert_no_work_dirs(&fx.resources());
}

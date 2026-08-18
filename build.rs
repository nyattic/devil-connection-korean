use std::path::PathBuf;

use dc_asar::{ArchiveRoot, PackOptions, create_archive_from};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    embed_windows_icon();

    if std::env::var_os("CARGO_FEATURE_EMBED_DATA").is_none() {
        return;
    }

    println!("cargo:rerun-if-env-changed=DC_DATA_DIR");

    let root = translation_root();
    let data = root.join("data");
    let tyrano = root.join("tyrano");

    for dir in [&data, &tyrano] {
        println!("cargo:rerun-if-changed={}", dir.display());
    }

    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("translation.asar");
    let roots = [
        ArchiveRoot {
            archive_path: "data",
            source: &data,
        },
        ArchiveRoot {
            archive_path: "tyrano",
            source: &tyrano,
        },
    ];
    let options = PackOptions {
        unpack: Vec::new(),
        ..PackOptions::default()
    };

    match create_archive_from(&roots, &dest, &options) {
        Ok(stats) => println!(
            "cargo:warning=번역 데이터 {}개 파일을 실행 파일에 포함합니다 ({}MB)",
            stats.files,
            stats.archive_bytes / (1024 * 1024)
        ),
        Err(e) => panic!("번역 데이터 아카이브 생성에 실패했습니다: {e}"),
    }
}

fn translation_root() -> PathBuf {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let candidates = match std::env::var_os("DC_DATA_DIR") {
        Some(dir) => vec![PathBuf::from(dir)],
        None => vec![manifest.clone(), manifest.join("../devil-connection-data")],
    };

    for candidate in &candidates {
        if candidate.join("data").is_dir() && candidate.join("tyrano").is_dir() {
            return candidate.clone();
        }
    }

    panic!(
        "번역 데이터를 찾지 못했습니다. DC_DATA_DIR로 data/와 tyrano/가 있는 폴더를 지정해주세요. 확인한 위치: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[cfg(windows)]
fn embed_windows_icon() {
    println!("cargo:rerun-if-changed=assets/icon.ico");

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icon.ico");
    if let Err(e) = resource.compile() {
        panic!("Windows 아이콘 리소스를 삽입하지 못했습니다: {e}");
    }
}

#[cfg(not(windows))]
fn embed_windows_icon() {}

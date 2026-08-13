use std::path::PathBuf;

use dc_asar::{ArchiveRoot, PackOptions, create_archive_from};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    embed_windows_icon();

    if std::env::var_os("CARGO_FEATURE_EMBED_DATA").is_none() {
        return;
    }

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let data = manifest.join("data");
    let tyrano = manifest.join("tyrano");

    for dir in [&data, &tyrano] {
        if !dir.is_dir() {
            panic!("번역 데이터 폴더가 없습니다: {}", dir.display());
        }
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

use std::fs;
use std::path::Path;

use dc_asar::{AsarArchive, EntryKind, PackOptions, create_archive};

const VIDEO_LEN: usize = 5 * 1024 * 1024 + 7;

fn build_source(root: &Path) {
    let write = |rel: &str, contents: &[u8]| {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    };

    write("index.html", b"<html>original</html>");
    write("package.json", br#"{"name":"devil","main":"index.js"}"#);
    write("data/scenario/first.ks", "원본 시나리오".as_bytes());
    write("data/scenario/scene1.ks", "원본 장면".as_bytes());
    write("data/others/master_data.js", b"var master = {};");
    write("tyrano/tyrano.css", b"body{}");
    write("data/video/title.mp4", &vec![0xABu8; VIDEO_LEN]);
    write("data/system/empty.tjs", b"");
    fs::create_dir_all(root.join("data/emptydir")).unwrap();
    write("bin/native.node", b"\x7fELF fake native module");
}

#[test]
fn pack_then_extract_reproduces_source_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let archive = tmp.path().join("app.asar");
    let out = tmp.path().join("out");

    build_source(&src);
    let stats = create_archive(&src, &archive, &PackOptions::default()).unwrap();

    assert_eq!(stats.unpacked_files, 1);
    assert!(
        dc_asar::unpacked_dir_for(&archive)
            .join("bin/native.node")
            .exists()
    );

    let mut opened = AsarArchive::open(&archive).unwrap();
    opened.validate().unwrap();
    opened.extract_to(&out).unwrap();

    for rel in [
        "index.html",
        "package.json",
        "data/scenario/first.ks",
        "data/scenario/scene1.ks",
        "data/others/master_data.js",
        "tyrano/tyrano.css",
        "data/video/title.mp4",
        "data/system/empty.tjs",
        "bin/native.node",
    ] {
        assert_eq!(
            fs::read(src.join(rel)).unwrap(),
            fs::read(out.join(rel)).unwrap(),
            "{rel} 내용이 다릅니다"
        );
    }

    assert!(
        out.join("data/emptydir").is_dir(),
        "빈 디렉터리가 유실됐습니다"
    );
}

#[test]
fn repack_after_overwrite_keeps_translation() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let original = tmp.path().join("app.asar");
    let work = tmp.path().join("work");
    let patched = tmp.path().join("app.asar.new");

    build_source(&src);
    create_archive(&src, &original, &PackOptions::default()).unwrap();

    AsarArchive::open(&original)
        .unwrap()
        .extract_to(&work)
        .unwrap();
    fs::write(
        work.join("data/scenario/first.ks"),
        "번역된 시나리오".as_bytes(),
    )
    .unwrap();
    fs::write(work.join("data/scenario/added.ks"), "새로 추가".as_bytes()).unwrap();
    create_archive(&work, &patched, &PackOptions::default()).unwrap();

    let mut archive = AsarArchive::open(&patched).unwrap();
    archive.validate().unwrap();

    assert_eq!(
        archive.read_file("data/scenario/first.ks").unwrap(),
        "번역된 시나리오".as_bytes()
    );
    assert_eq!(
        archive.read_file("data/scenario/added.ks").unwrap(),
        "새로 추가".as_bytes()
    );
    assert_eq!(
        archive.read_file("data/others/master_data.js").unwrap(),
        b"var master = {};"
    );
    assert_eq!(
        archive.read_file("data/video/title.mp4").unwrap().len(),
        VIDEO_LEN
    );
}

#[test]
fn packing_is_deterministic() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    build_source(&src);

    let first = tmp.path().join("a.asar");
    let second = tmp.path().join("b.asar");
    create_archive(&src, &first, &PackOptions::default()).unwrap();
    create_archive(&src, &second, &PackOptions::default()).unwrap();

    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
}

#[test]
fn integrity_blocks_match_file_contents() {
    use sha2::{Digest, Sha256};

    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let archive = tmp.path().join("app.asar");
    build_source(&src);

    let options = PackOptions {
        integrity: true,
        ..PackOptions::default()
    };
    create_archive(&src, &archive, &options).unwrap();

    let opened = AsarArchive::open(&archive).unwrap();
    let node = opened.root().lookup("data/video/title.mp4").unwrap();
    let integrity = node.integrity.as_ref().expect("integrity가 없습니다");

    let contents = fs::read(src.join("data/video/title.mp4")).unwrap();
    assert_eq!(integrity.algorithm, "SHA256");
    assert_eq!(integrity.hash, hex::encode(Sha256::digest(&contents)));
    assert_eq!(integrity.block_size, dc_asar::INTEGRITY_BLOCK_SIZE);
    assert_eq!(
        integrity.blocks.len(),
        contents.len().div_ceil(dc_asar::INTEGRITY_BLOCK_SIZE)
    );
    assert!(
        integrity.blocks.len() >= 2,
        "다중 블록을 검증하지 못했습니다"
    );
    for (index, block) in integrity.blocks.iter().enumerate() {
        let start = index * dc_asar::INTEGRITY_BLOCK_SIZE;
        let end = (start + dc_asar::INTEGRITY_BLOCK_SIZE).min(contents.len());
        assert_eq!(
            *block,
            hex::encode(Sha256::digest(&contents[start..end])),
            "블록 {index}"
        );
    }
}

#[test]
fn hash_file_matches_source() {
    use sha2::{Digest, Sha256};

    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let archive = tmp.path().join("app.asar");
    build_source(&src);
    create_archive(&src, &archive, &PackOptions::default()).unwrap();

    let mut opened = AsarArchive::open(&archive).unwrap();
    for rel in [
        "data/scenario/first.ks",
        "data/video/title.mp4",
        "bin/native.node",
    ] {
        let expected: [u8; 32] = Sha256::digest(fs::read(src.join(rel)).unwrap()).into();
        assert_eq!(opened.hash_file(rel).unwrap(), expected, "{rel}");
    }
}

#[test]
fn entries_report_expected_kinds() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let archive = tmp.path().join("app.asar");
    build_source(&src);
    create_archive(&src, &archive, &PackOptions::default()).unwrap();

    let opened = AsarArchive::open(&archive).unwrap();
    let entries = opened.entries();

    let native = entries
        .iter()
        .find(|e| e.path == "bin/native.node")
        .expect("native.node 항목이 없습니다");
    assert!(matches!(
        native.kind,
        EntryKind::File { unpacked: true, .. }
    ));

    let scenario = entries
        .iter()
        .find(|e| e.path == "data/scenario")
        .expect("data/scenario 항목이 없습니다");
    assert!(matches!(scenario.kind, EntryKind::Directory));

    let dir_index = entries.iter().position(|e| e.path == "data").unwrap();
    let child_index = entries
        .iter()
        .position(|e| e.path == "data/scenario/first.ks")
        .unwrap();
    assert!(dir_index < child_index);
}

#[cfg(unix)]
#[test]
fn symlinks_survive_a_roundtrip() {
    use std::os::unix::fs::symlink;

    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let archive = tmp.path().join("app.asar");
    let out = tmp.path().join("out");

    fs::create_dir_all(src.join("data/scenario")).unwrap();
    fs::write(src.join("data/target.ks"), "대상").unwrap();
    symlink("../target.ks", src.join("data/scenario/alias.ks")).unwrap();

    create_archive(&src, &archive, &PackOptions::default()).unwrap();
    let mut opened = AsarArchive::open(&archive).unwrap();
    opened.validate().unwrap();
    opened.extract_to(&out).unwrap();

    let link = fs::read_link(out.join("data/scenario/alias.ks")).unwrap();
    assert_eq!(link.to_str().unwrap(), "../target.ks");
    assert_eq!(
        fs::read_to_string(out.join("data/scenario/alias.ks")).unwrap(),
        "대상"
    );
}

#[cfg(unix)]
#[test]
fn executable_bit_survives_a_roundtrip() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let archive = tmp.path().join("app.asar");
    let out = tmp.path().join("out");

    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("run.sh"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(src.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(src.join("plain.txt"), "x").unwrap();
    fs::set_permissions(src.join("plain.txt"), fs::Permissions::from_mode(0o644)).unwrap();

    create_archive(&src, &archive, &PackOptions::default()).unwrap();
    AsarArchive::open(&archive)
        .unwrap()
        .extract_to(&out)
        .unwrap();

    assert_ne!(
        fs::metadata(out.join("run.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    assert_eq!(
        fs::metadata(out.join("plain.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}

#[test]
fn validate_rejects_out_of_range_offsets() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let archive = tmp.path().join("app.asar");
    build_source(&src);
    create_archive(&src, &archive, &PackOptions::default()).unwrap();

    let mut bytes = fs::read(&archive).unwrap();
    bytes.truncate(bytes.len() - 1024);
    fs::write(&archive, &bytes).unwrap();

    let opened = AsarArchive::open(&archive).unwrap();
    assert!(opened.validate().is_err(), "잘린 아카이브를 통과시켰습니다");
}

#[test]
fn open_rejects_non_asar_files() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("not-an-asar");
    fs::write(&path, b"this is definitely not an asar archive").unwrap();
    assert!(AsarArchive::open(&path).is_err());
    assert!(!dc_asar::looks_like_asar(
        b"this is definitely not an asar archive"
    ));
}

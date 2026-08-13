use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{AsarError, Result};
use crate::header::{self, Integrity, Node, INTEGRITY_BLOCK_SIZE};
use crate::pattern;
use crate::read::unpacked_dir_for;
use crate::safepath;

const COPY_CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PackOptions {
    pub unpack: Vec<String>,
    pub integrity: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        PackOptions {
            unpack: vec!["*.node".to_string()],
            integrity: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackStats {
    pub directories: u64,
    pub files: u64,
    pub links: u64,
    pub unpacked_files: u64,
    pub bytes: u64,
    pub archive_bytes: u64,
}

#[derive(Debug)]
struct SourceEntry {
    rel_path: String,
    source: PathBuf,
    kind: SourceKind,
}

#[derive(Debug)]
enum SourceKind {
    Directory,
    Link { target: String },
    File { size: u64, executable: bool },
}

pub fn create_archive(
    src_dir: impl AsRef<Path>,
    dest: impl AsRef<Path>,
    options: &PackOptions,
) -> Result<PackStats> {
    let src_dir = src_dir.as_ref();
    let dest = dest.as_ref();

    let mut entries = Vec::new();
    collect(src_dir, "", &mut entries)?;
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    let unpacked_dir = unpacked_dir_for(dest);
    let mut stats = PackStats::default();
    let mut root = Node::directory();
    let mut packed: Vec<(String, PathBuf, u64)> = Vec::new();
    let mut offset: u64 = 0;

    for entry in &entries {
        match &entry.kind {
            SourceKind::Directory => {
                ensure_directory_node(&mut root, &entry.rel_path)?;
                stats.directories += 1;
            }
            SourceKind::Link { target } => {
                let dir = entry
                    .rel_path
                    .rsplit_once('/')
                    .map(|(d, _)| d)
                    .unwrap_or("");
                safepath::check_link_target(dir, target)?;
                root.insert(
                    &entry.rel_path,
                    Node {
                        link: Some(target.clone()),
                        ..Default::default()
                    },
                )?;
                stats.links += 1;
            }
            SourceKind::File { size, executable } => {
                let unpacked = pattern::matches_any(&options.unpack, &entry.rel_path);
                let integrity = if options.integrity {
                    Some(compute_integrity(&entry.source)?)
                } else {
                    None
                };

                let node = if unpacked {
                    let target = safepath::join_checked(&unpacked_dir, &entry.rel_path)?;
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(|e| AsarError::io(parent, e))?;
                    }
                    fs::copy(&entry.source, &target)
                        .map_err(|e| AsarError::io(&entry.source, e))?;
                    stats.unpacked_files += 1;

                    Node {
                        size: Some(*size),
                        unpacked: Some(true),
                        executable: executable.then_some(true),
                        integrity,
                        ..Default::default()
                    }
                } else {
                    packed.push((entry.rel_path.clone(), entry.source.clone(), *size));
                    let node = Node {
                        size: Some(*size),
                        offset: Some(offset.to_string()),
                        executable: executable.then_some(true),
                        integrity,
                        ..Default::default()
                    };
                    offset = offset.checked_add(*size).ok_or_else(|| {
                        AsarError::Header("아카이브 크기가 u64 범위를 넘었습니다".to_string())
                    })?;
                    node
                };

                root.insert(&entry.rel_path, node)?;
                stats.files += 1;
                stats.bytes += *size;
            }
        }
    }

    let header_bytes = header::serialize(&root)?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| AsarError::io(parent, e))?;
    }
    let file = File::create(dest).map_err(|e| AsarError::io(dest, e))?;
    let mut writer = BufWriter::with_capacity(COPY_CHUNK, file);
    writer
        .write_all(&header_bytes)
        .map_err(|e| AsarError::io(dest, e))?;

    let mut buf = vec![0u8; COPY_CHUNK];
    for (rel_path, source, expected_size) in &packed {
        let mut input = File::open(source).map_err(|e| AsarError::io(source, e))?;
        let mut written = 0u64;

        loop {
            let read = input.read(&mut buf).map_err(|e| AsarError::io(source, e))?;
            if read == 0 {
                break;
            }
            writer
                .write_all(&buf[..read])
                .map_err(|e| AsarError::io(dest, e))?;
            written += read as u64;
        }

        if written != *expected_size {
            return Err(AsarError::SizeMismatch {
                path: rel_path.clone(),
                expected: *expected_size,
                actual: written,
            });
        }
    }

    writer.flush().map_err(|e| AsarError::io(dest, e))?;
    let file = writer
        .into_inner()
        .map_err(|e| AsarError::io(dest, e.into_error()))?;
    file.sync_all().map_err(|e| AsarError::io(dest, e))?;

    stats.archive_bytes = header_bytes.len() as u64 + offset;
    Ok(stats)
}

fn collect(dir: &Path, prefix: &str, out: &mut Vec<SourceEntry>) -> Result<()> {
    let read_dir = fs::read_dir(dir).map_err(|e| AsarError::io(dir, e))?;

    for item in read_dir {
        let item = item.map_err(|e| AsarError::io(dir, e))?;
        let path = item.path();

        let name = item.file_name();
        let Some(name) = name.to_str() else {
            return Err(AsarError::UnsafePath(format!(
                "{} (UTF-8이 아닌 파일 이름)",
                path.display()
            )));
        };
        safepath::check_component(name)?;

        let rel_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        let metadata = fs::symlink_metadata(&path).map_err(|e| AsarError::io(&path, e))?;

        if metadata.is_symlink() {
            let target = fs::read_link(&path).map_err(|e| AsarError::io(&path, e))?;
            let Some(target) = target.to_str() else {
                return Err(AsarError::UnsafePath(format!(
                    "{} (UTF-8이 아닌 링크 대상)",
                    path.display()
                )));
            };
            out.push(SourceEntry {
                rel_path,
                source: path,
                kind: SourceKind::Link {
                    target: target.replace('\\', "/"),
                },
            });
        } else if metadata.is_dir() {
            out.push(SourceEntry {
                rel_path: rel_path.clone(),
                source: path.clone(),
                kind: SourceKind::Directory,
            });
            collect(&path, &rel_path, out)?;
        } else if metadata.is_file() {
            out.push(SourceEntry {
                rel_path,
                source: path,
                kind: SourceKind::File {
                    size: metadata.len(),
                    executable: is_executable(&metadata),
                },
            });
        } else {
            return Err(AsarError::UnsupportedEntry(path.display().to_string()));
        }
    }

    Ok(())
}

fn ensure_directory_node(root: &mut Node, rel_path: &str) -> Result<()> {
    let mut current = root;
    for segment in rel_path.split('/').filter(|s| !s.is_empty()) {
        current = current
            .files
            .get_or_insert_with(BTreeMap::new)
            .entry(segment.to_string())
            .or_insert_with(Node::directory);
        if !current.is_directory() {
            return Err(AsarError::UnsafePath(rel_path.to_string()));
        }
    }
    Ok(())
}

fn compute_integrity(path: &Path) -> Result<Integrity> {
    let mut file = File::open(path).map_err(|e| AsarError::io(path, e))?;
    let mut whole = Sha256::new();
    let mut blocks = Vec::new();
    let mut buf = vec![0u8; INTEGRITY_BLOCK_SIZE];

    loop {
        let mut filled = 0usize;
        while filled < buf.len() {
            let read = file
                .read(&mut buf[filled..])
                .map_err(|e| AsarError::io(path, e))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled == 0 {
            break;
        }

        whole.update(&buf[..filled]);
        blocks.push(hex::encode(Sha256::digest(&buf[..filled])));

        if filled < buf.len() {
            break;
        }
    }

    Ok(Integrity {
        algorithm: "SHA256".to_string(),
        hash: hex::encode(whole.finalize()),
        block_size: INTEGRITY_BLOCK_SIZE,
        blocks,
    })
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

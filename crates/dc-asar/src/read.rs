use std::fs::{self, File};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{AsarError, Result};
use crate::header::{self, Entry, EntryKind, Node};
use crate::safepath;

const COPY_CHUNK: usize = 1024 * 1024;

const MAX_IN_MEMORY_FILE: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractStats {
    pub directories: u64,
    pub files: u64,
    pub links: u64,
    pub unpacked_files: u64,
    pub bytes: u64,
}

pub struct AsarArchive<R = BufReader<File>> {
    path: PathBuf,
    unpacked_dir: PathBuf,
    root: Node,
    data_offset: u64,
    archive_len: u64,
    reader: R,
}

impl AsarArchive<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(|e| AsarError::io(&path, e))?;
        let archive_len = file.metadata().map_err(|e| AsarError::io(&path, e))?.len();

        let mut reader = BufReader::new(file);
        let (root, data_offset) = header::read(&mut reader)?;

        Ok(AsarArchive {
            unpacked_dir: unpacked_dir_for(&path),
            path,
            root,
            data_offset,
            archive_len,
            reader,
        })
    }
}

impl<'a> AsarArchive<Cursor<&'a [u8]>> {
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        let mut reader = Cursor::new(bytes);
        let (root, data_offset) = header::read(&mut reader)?;

        if let Some(entry) = header::flatten(&root)
            .into_iter()
            .find(|entry| matches!(entry.kind, EntryKind::File { unpacked: true, .. }))
        {
            return Err(AsarError::UnsupportedEntry(format!(
                "{} (메모리 아카이브에는 unpacked 항목을 담을 수 없습니다)",
                entry.path
            )));
        }

        Ok(AsarArchive {
            path: PathBuf::new(),
            unpacked_dir: PathBuf::new(),
            root,
            data_offset,
            archive_len: bytes.len() as u64,
            reader,
        })
    }
}

impl<R: Read + Seek> AsarArchive<R> {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn unpacked_dir(&self) -> &Path {
        &self.unpacked_dir
    }

    pub fn root(&self) -> &Node {
        &self.root
    }

    pub fn data_offset(&self) -> u64 {
        self.data_offset
    }

    pub fn entries(&self) -> Vec<Entry> {
        header::flatten(&self.root)
    }

    pub fn validate(&self) -> Result<()> {
        for entry in self.entries() {
            safepath::check_rel_path(&entry.path)?;
            match &entry.kind {
                EntryKind::Directory => {}
                EntryKind::Link { target } => {
                    let dir = entry.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    safepath::check_link_target(dir, target)?;
                }
                EntryKind::File {
                    size,
                    offset,
                    unpacked,
                    ..
                } => {
                    if *unpacked {
                        continue;
                    }
                    let offset = offset.ok_or_else(|| {
                        AsarError::Header(format!("'{}'에 offset이 없습니다", entry.path))
                    })?;
                    let end = self
                        .data_offset
                        .checked_add(offset)
                        .and_then(|start| start.checked_add(*size))
                        .ok_or_else(|| {
                            AsarError::Header(format!("'{}'의 범위가 오버플로합니다", entry.path))
                        })?;
                    if end > self.archive_len {
                        return Err(AsarError::Header(format!(
                            "'{}'이 아카이브 끝을 넘어갑니다 ({end} > {})",
                            entry.path, self.archive_len
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn read_file(&mut self, rel_path: &str) -> Result<Vec<u8>> {
        let node = self
            .root
            .lookup(rel_path)
            .ok_or_else(|| AsarError::NotFound(rel_path.to_string()))?;

        if !node.is_file() {
            return Err(AsarError::UnsupportedEntry(rel_path.to_string()));
        }

        let size = node.size.unwrap_or(0);
        if size > MAX_IN_MEMORY_FILE {
            return Err(AsarError::UnsupportedEntry(format!(
                "{rel_path} (메모리 읽기 상한 초과: {size}바이트)"
            )));
        }

        if node.is_unpacked() {
            let path = safepath::join_checked(&self.unpacked_dir, rel_path)?;
            return fs::read(&path).map_err(|e| AsarError::io(&path, e));
        }

        let offset = node.parsed_offset().ok_or_else(|| {
            AsarError::Header(format!("'{rel_path}'의 offset을 해석할 수 없습니다"))
        })?;

        let mut buf = vec![0u8; size as usize];
        self.reader
            .seek(SeekFrom::Start(self.data_offset + offset))?;
        self.reader.read_exact(&mut buf)?;
        Ok(buf)
    }

    pub fn hash_file(&mut self, rel_path: &str) -> Result<[u8; 32]> {
        let node = self
            .root
            .lookup(rel_path)
            .ok_or_else(|| AsarError::NotFound(rel_path.to_string()))?
            .clone();

        if !node.is_file() {
            return Err(AsarError::UnsupportedEntry(rel_path.to_string()));
        }

        let size = node.size.unwrap_or(0);
        let mut hasher = Sha256::new();

        if node.is_unpacked() {
            let path = safepath::join_checked(&self.unpacked_dir, rel_path)?;
            let mut file = File::open(&path).map_err(|e| AsarError::io(&path, e))?;
            copy_into(&mut file, &mut HashWriter(&mut hasher), u64::MAX)?;
        } else {
            let offset = node.parsed_offset().ok_or_else(|| {
                AsarError::Header(format!("'{rel_path}'의 offset을 해석할 수 없습니다"))
            })?;
            self.reader
                .seek(SeekFrom::Start(self.data_offset + offset))?;
            copy_into(&mut self.reader, &mut HashWriter(&mut hasher), size)?;
        }

        Ok(hasher.finalize().into())
    }

    pub fn extract_to(&mut self, dest: impl AsRef<Path>) -> Result<ExtractStats> {
        let dest = dest.as_ref();
        fs::create_dir_all(dest).map_err(|e| AsarError::io(dest, e))?;

        let entries = self.entries();
        let mut stats = ExtractStats::default();

        for entry in &entries {
            let out_path = safepath::join_checked(dest, &entry.path)?;

            match &entry.kind {
                EntryKind::Directory => {
                    fs::create_dir_all(&out_path).map_err(|e| AsarError::io(&out_path, e))?;
                    stats.directories += 1;
                }
                EntryKind::Link { target } => {
                    let dir = entry.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                    safepath::check_link_target(dir, target)?;
                    create_parent(&out_path)?;
                    create_link(target, &out_path)?;
                    stats.links += 1;
                }
                EntryKind::File {
                    size,
                    offset,
                    unpacked,
                    executable,
                    ..
                } => {
                    create_parent(&out_path)?;

                    if *unpacked {
                        let src = safepath::join_checked(&self.unpacked_dir, &entry.path)?;
                        fs::copy(&src, &out_path).map_err(|e| AsarError::io(&src, e))?;
                        stats.unpacked_files += 1;
                    } else {
                        let offset = offset.ok_or_else(|| {
                            AsarError::Header(format!("'{}'에 offset이 없습니다", entry.path))
                        })?;
                        self.reader
                            .seek(SeekFrom::Start(self.data_offset + offset))?;
                        let mut out =
                            File::create(&out_path).map_err(|e| AsarError::io(&out_path, e))?;
                        let written = copy_into(&mut self.reader, &mut out, *size)?;
                        if written != *size {
                            return Err(AsarError::SizeMismatch {
                                path: entry.path.clone(),
                                expected: *size,
                                actual: written,
                            });
                        }
                        out.flush().map_err(|e| AsarError::io(&out_path, e))?;
                    }

                    set_executable(&out_path, *executable)?;
                    stats.files += 1;
                    stats.bytes += *size;
                }
            }
        }

        Ok(stats)
    }
}

pub fn unpacked_dir_for(archive: &Path) -> PathBuf {
    let mut name = archive.file_name().unwrap_or_default().to_os_string();
    name.push(".unpacked");
    archive.with_file_name(name)
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AsarError::io(parent, e))?;
    }
    Ok(())
}

struct HashWriter<'a>(&'a mut Sha256);

impl Write for HashWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn copy_into<R: Read + ?Sized, W: Write + ?Sized>(
    reader: &mut R,
    writer: &mut W,
    limit: u64,
) -> Result<u64> {
    let mut buf = vec![0u8; COPY_CHUNK];
    let mut remaining = limit;
    let mut total = 0u64;

    while remaining > 0 {
        let want = remaining.min(COPY_CHUNK as u64) as usize;
        let read = reader.read(&mut buf[..want])?;
        if read == 0 {
            break;
        }
        writer.write_all(&buf[..read])?;
        total += read as u64;
        remaining -= read as u64;
    }

    Ok(total)
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if !executable {
        return Ok(());
    }
    let metadata = fs::metadata(path).map_err(|e| AsarError::io(path, e))?;
    let mut perms = metadata.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms).map_err(|e| AsarError::io(path, e))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn create_link(target: &str, out_path: &Path) -> Result<()> {
    if out_path.symlink_metadata().is_ok() {
        fs::remove_file(out_path).map_err(|e| AsarError::io(out_path, e))?;
    }
    std::os::unix::fs::symlink(target, out_path).map_err(|e| AsarError::io(out_path, e))
}

#[cfg(not(unix))]
fn create_link(target: &str, out_path: &Path) -> Result<()> {
    let parent = out_path.parent().unwrap_or_else(|| Path::new("."));
    let mut src = parent.to_path_buf();
    for segment in target.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                src.pop();
            }
            other => src.push(other),
        }
    }
    fs::copy(&src, out_path).map_err(|e| AsarError::io(&src, e))?;
    Ok(())
}

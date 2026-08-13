use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::error::{AsarError, Result};

pub const SIZE_PICKLE_LEN: u64 = 8;

const MAX_HEADER_LEN: u32 = 256 * 1024 * 1024;

pub const INTEGRITY_BLOCK_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Integrity {
    pub algorithm: String,
    pub hash: String,
    #[serde(rename = "blockSize")]
    pub block_size: usize,
    pub blocks: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<String, Node>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unpacked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<Integrity>,
}

impl Node {
    pub fn directory() -> Self {
        Node {
            files: Some(BTreeMap::new()),
            ..Default::default()
        }
    }

    pub fn is_directory(&self) -> bool {
        self.files.is_some()
    }

    pub fn is_link(&self) -> bool {
        self.link.is_some()
    }

    pub fn is_file(&self) -> bool {
        !self.is_directory() && !self.is_link()
    }

    pub fn is_unpacked(&self) -> bool {
        self.unpacked.unwrap_or(false)
    }

    pub fn parsed_offset(&self) -> Option<u64> {
        self.offset.as_ref().and_then(|s| s.parse::<u64>().ok())
    }

    pub fn lookup(&self, rel_path: &str) -> Option<&Node> {
        let mut current = self;
        for segment in rel_path.split('/').filter(|s| !s.is_empty()) {
            current = current.files.as_ref()?.get(segment)?;
        }
        Some(current)
    }

    pub fn insert(&mut self, rel_path: &str, node: Node) -> Result<()> {
        let segments: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
        let (name, dirs) = match segments.split_last() {
            Some(parts) => parts,
            None => return Err(AsarError::UnsafePath(rel_path.to_string())),
        };

        let mut current = self;
        for dir in dirs {
            current = current
                .files
                .get_or_insert_with(BTreeMap::new)
                .entry((*dir).to_string())
                .or_insert_with(Node::directory);
            if !current.is_directory() {
                return Err(AsarError::UnsafePath(rel_path.to_string()));
            }
        }

        current
            .files
            .get_or_insert_with(BTreeMap::new)
            .insert((*name).to_string(), node);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    Link {
        target: String,
    },
    File {
        size: u64,
        offset: Option<u64>,
        unpacked: bool,
        executable: bool,
        integrity: Option<Integrity>,
    },
}

pub fn flatten(root: &Node) -> Vec<Entry> {
    let mut out = Vec::new();
    flatten_into(root, "", &mut out);
    out
}

fn flatten_into(node: &Node, prefix: &str, out: &mut Vec<Entry>) {
    let Some(files) = node.files.as_ref() else {
        return;
    };

    for (name, child) in files {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        if child.is_directory() {
            out.push(Entry {
                path: path.clone(),
                kind: EntryKind::Directory,
            });
            flatten_into(child, &path, out);
        } else if let Some(link) = child.link.as_ref() {
            out.push(Entry {
                path,
                kind: EntryKind::Link {
                    target: link.clone(),
                },
            });
        } else {
            out.push(Entry {
                path,
                kind: EntryKind::File {
                    size: child.size.unwrap_or(0),
                    offset: child.parsed_offset(),
                    unpacked: child.is_unpacked(),
                    executable: child.executable.unwrap_or(false),
                    integrity: child.integrity.clone(),
                },
            });
        }
    }
}

pub fn read<R: Read>(reader: &mut R) -> Result<(Node, u64)> {
    let mut size_buf = [0u8; SIZE_PICKLE_LEN as usize];
    reader.read_exact(&mut size_buf)?;

    let size_payload_len = u32::from_le_bytes(size_buf[0..4].try_into().unwrap());
    if size_payload_len != 4 {
        return Err(AsarError::Header(format!(
            "크기 Pickle 길이가 4가 아닙니다 ({size_payload_len})"
        )));
    }

    let header_len = u32::from_le_bytes(size_buf[4..8].try_into().unwrap());
    if header_len < 8 {
        return Err(AsarError::Header(format!(
            "헤더 길이가 너무 짧습니다 ({header_len})"
        )));
    }
    if header_len > MAX_HEADER_LEN {
        return Err(AsarError::Header(format!(
            "헤더 길이가 상한을 초과했습니다 ({header_len})"
        )));
    }

    let mut header_buf = vec![0u8; header_len as usize];
    reader.read_exact(&mut header_buf)?;

    let payload_len = u32::from_le_bytes(header_buf[0..4].try_into().unwrap());
    if payload_len as u64 + 4 != header_len as u64 {
        return Err(AsarError::Header(format!(
            "헤더 Pickle 페이로드 길이가 맞지 않습니다 ({payload_len} vs {header_len})"
        )));
    }

    let json_len = u32::from_le_bytes(header_buf[4..8].try_into().unwrap()) as usize;
    let json_end = 8usize
        .checked_add(json_len)
        .ok_or_else(|| AsarError::Header("헤더 JSON 길이가 유효하지 않습니다".to_string()))?;
    if json_end > header_buf.len() {
        return Err(AsarError::Header(format!(
            "헤더 JSON이 버퍼 범위를 벗어납니다 ({json_end} > {})",
            header_buf.len()
        )));
    }

    let root: Node = serde_json::from_slice(&header_buf[8..json_end])?;
    if !root.is_directory() {
        return Err(AsarError::Header(
            "헤더 루트에 files 항목이 없습니다".to_string(),
        ));
    }

    Ok((root, SIZE_PICKLE_LEN + header_len as u64))
}

pub fn serialize(root: &Node) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(root)?;
    let json_len = json.len();
    let padding = (4 - (json_len % 4)) % 4;
    let payload_len = 4 + json_len + padding;
    let header_len = 4 + payload_len;

    if header_len > MAX_HEADER_LEN as usize {
        return Err(AsarError::Header(format!(
            "헤더가 너무 큽니다 ({header_len}바이트)"
        )));
    }

    let mut out = Vec::with_capacity(SIZE_PICKLE_LEN as usize + header_len);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&(header_len as u32).to_le_bytes());
    out.extend_from_slice(&(payload_len as u32).to_le_bytes());
    out.extend_from_slice(&(json_len as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.resize(out.len() + padding, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_root() -> Node {
        let mut root = Node::directory();
        root.insert(
            "data/scenario/first.ks",
            Node {
                size: Some(1234),
                offset: Some("0".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        root.insert(
            "native.node",
            Node {
                size: Some(10),
                unpacked: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        root
    }

    #[test]
    fn header_roundtrip() {
        let root = sample_root();
        let bytes = serialize(&root).unwrap();
        let (parsed, data_offset) = read(&mut bytes.as_slice()).unwrap();
        assert_eq!(parsed, root);
        assert_eq!(data_offset, bytes.len() as u64);
    }

    #[test]
    fn header_is_four_byte_aligned() {
        for extra in 0..8 {
            let mut root = Node::directory();
            root.insert(
                &format!("a{}", "b".repeat(extra)),
                Node {
                    size: Some(1),
                    offset: Some("0".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();
            let bytes = serialize(&root).unwrap();
            assert_eq!(bytes.len() % 4, 0, "extra={extra}");
            let (parsed, _) = read(&mut bytes.as_slice()).unwrap();
            assert_eq!(parsed, root);
        }
    }

    #[test]
    fn flatten_lists_directories_before_children() {
        let entries = flatten(&sample_root());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "data",
                "data/scenario",
                "data/scenario/first.ks",
                "native.node"
            ]
        );
        assert!(matches!(entries[0].kind, EntryKind::Directory));
        assert!(matches!(
            entries[3].kind,
            EntryKind::File {
                unpacked: true,
                offset: None,
                ..
            }
        ));
    }

    #[test]
    fn lookup_finds_nested_node() {
        let root = sample_root();
        assert_eq!(
            root.lookup("data/scenario/first.ks").unwrap().size,
            Some(1234)
        );
        assert!(root.lookup("data/scenario/missing.ks").is_none());
    }

    #[test]
    fn rejects_truncated_and_malformed_headers() {
        assert!(read(&mut [0u8; 4].as_slice()).is_err());

        let mut bad = serialize(&sample_root()).unwrap();
        bad[0] = 5;
        assert!(read(&mut bad.as_slice()).is_err());

        let mut bad = serialize(&sample_root()).unwrap();
        bad[4] = bad[4].wrapping_add(1);
        assert!(read(&mut bad.as_slice()).is_err());
    }
}

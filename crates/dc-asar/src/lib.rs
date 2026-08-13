mod error;
mod header;
mod pattern;
mod read;
mod safepath;
mod write;

pub use error::{AsarError, Result};
pub use header::{flatten, Entry, EntryKind, Integrity, Node, INTEGRITY_BLOCK_SIZE};
pub use read::{unpacked_dir_for, AsarArchive, ExtractStats};
pub use write::{create_archive, PackOptions, PackStats};

pub fn looks_like_asar(bytes: &[u8]) -> bool {
    if bytes.len() < 16 {
        return false;
    }
    let size_payload_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let header_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let payload_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    size_payload_len == 4 && header_len >= 8 && payload_len as u64 + 4 == header_len as u64
}

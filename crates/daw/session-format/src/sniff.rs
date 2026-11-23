//! What a song's file is, from its first bytes, never from its name.
//!
//! A `.session` has been three things: a folder (the styx manifest and
//! `objects/`, [`crate::DawProject`]), a Turso store (the engine's project,
//! [`crate::store`]) and now the store holding the whole song. Beside it,
//! until a folder is converted, sits the session primitive's own document
//! (`<Title>.fts-session`, an Architect document file). A reader asks
//! [`sniff`] which it holds, and opens it accordingly.

/// What a SQLite file starts with: the one-file song (and the engine's
/// project before it).
pub const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// What an Architect document file starts with: the session primitive's
/// document as a file of its own (`.fts-session`), before it moved into
/// the song's one file.
pub const ARCHDOC_MAGIC: &[u8; 8] = b"ARCHDOC\0";

/// What a file holds, by its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileFormat {
    /// A Turso (SQLite) store: a song's one file.
    Store,
    /// An Architect document file: a `.fts-session` of the old pair.
    DocFile,
    /// Anything else (a styx manifest, nothing at all).
    Other,
}

/// What the file starting with `head` (its first 16 bytes or more) holds.
#[must_use]
pub fn sniff(head: &[u8]) -> FileFormat {
    if head.starts_with(SQLITE_MAGIC) {
        FileFormat::Store
    } else if head.starts_with(ARCHDOC_MAGIC) {
        FileFormat::DocFile
    } else {
        FileFormat::Other
    }
}

/// What the file at `path` holds: [`sniff`] of its first bytes. A folder,
/// or a file that will not open, is [`FileFormat::Other`].
#[must_use]
pub fn sniff_path(path: &std::path::Path) -> FileFormat {
    use std::io::Read as _;
    let mut head = [0_u8; 16];
    let Ok(mut file) = std::fs::File::open(path) else {
        return FileFormat::Other;
    };
    let mut n = 0;
    while n < head.len() {
        match file.read(&mut head[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    sniff(&head[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_say_what_a_file_is() {
        assert_eq!(sniff(b"SQLite format 3\0rest"), FileFormat::Store);
        assert_eq!(sniff(b"ARCHDOC\0\x01\0\0\0"), FileFormat::DocFile);
        assert_eq!(sniff(b"name Drive To LA"), FileFormat::Other);
        assert_eq!(sniff(b""), FileFormat::Other);
        assert_eq!(sniff(b"SQLite"), FileFormat::Other);
    }
}

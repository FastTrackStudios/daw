//! CFB compound file loader.
//!
//! Reads an entire AAF compound file into memory, indexing every
//! `"properties"` and `"index"` stream by their parent CFB directory path.
//! All directory (storage) paths are collected for parent→child navigation.

use crate::error::{AafError, AafResult};
use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// In-memory index of an AAF compound file.
pub struct CfbStore {
    /// `dir_path → raw bytes of "properties" stream`
    properties: HashMap<PathBuf, Vec<u8>>,
    /// `collection_dir_path → raw bytes of "index" stream`
    index: HashMap<PathBuf, Vec<u8>>,
    /// Every CFB storage (directory) path in the file.
    all_dirs: HashSet<PathBuf>,
}

impl CfbStore {
    /// Open and fully index an AAF/CFB file.
    pub fn load(path: &Path) -> AafResult<Self> {
        let mut compound = cfb::open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::InvalidData {
                AafError::InvalidFile {
                    reason: e.to_string(),
                }
            } else {
                AafError::Io(e)
            }
        })?;

        // Collect all entry metadata before borrowing compound mutably for
        // stream reads (the two operations can't overlap).
        let entries: Vec<(PathBuf, bool, bool)> = compound
            .walk()
            .map(|e| (e.path().to_path_buf(), e.is_storage(), e.is_stream()))
            .collect();

        let mut properties: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        let mut index: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        let mut all_dirs: HashSet<PathBuf> = HashSet::new();

        for (path, is_storage, is_stream) in entries {
            if is_storage {
                all_dirs.insert(path);
                continue;
            }
            if !is_stream {
                continue;
            }

            let stem = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            let parent = path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("/"));

            match stem {
                "properties" => {
                    let mut buf = Vec::new();
                    if compound
                        .open_stream(&path)
                        .and_then(|mut s| s.read_to_end(&mut buf))
                        .is_ok()
                    {
                        properties.insert(parent, buf);
                    }
                }
                "index" => {
                    let mut buf = Vec::new();
                    if compound
                        .open_stream(&path)
                        .and_then(|mut s| s.read_to_end(&mut buf))
                        .is_ok()
                    {
                        index.insert(parent, buf);
                    }
                }
                _ => {} // data streams and other streams are not needed for parsing
            }
        }

        Ok(Self {
            properties,
            index,
            all_dirs,
        })
    }

    // ─── Data access ─────────────────────────────────────────────────────────

    /// Raw bytes of the `"properties"` stream for the object at `dir_path`.
    pub fn properties(&self, dir_path: &Path) -> Option<&[u8]> {
        self.properties.get(dir_path).map(Vec::as_slice)
    }

    /// Raw bytes of the `"index"` stream for a collection directory.
    pub fn index_bytes(&self, coll_dir: &Path) -> Option<&[u8]> {
        self.index.get(coll_dir).map(Vec::as_slice)
    }

    // ─── Directory navigation ─────────────────────────────────────────────────

    /// List all direct child **directories** of `parent`.
    ///
    /// "Index" streams and "properties" streams are *streams* (not storages),
    /// so they never appear here; this returns only object/collection
    /// sub-directories.
    pub fn child_dirs(&self, parent: &Path) -> Vec<PathBuf> {
        self.all_dirs
            .iter()
            .filter(|p| p.parent().map(|par| par == parent).unwrap_or(false))
            .cloned()
            .collect()
    }

    /// Child directories sorted by name (lexicographic, so hex keys sort in
    /// insertion order: `00000000`, `00000001`, …).
    pub fn child_dirs_sorted(&self, parent: &Path) -> Vec<PathBuf> {
        let mut dirs = self.child_dirs(parent);
        dirs.sort();
        dirs
    }

    // ─── Collection helpers ───────────────────────────────────────────────────

    /// Ordered element paths for a **strong reference vector** collection
    /// directory.
    ///
    /// Reads the `"index"` stream (format: `u32 count + N × u32 local_key`)
    /// and returns paths in key order.  Falls back to sorted child directories
    /// if the index stream is missing or malformed.
    pub fn vector_elements(&self, coll_dir: &Path) -> Vec<PathBuf> {
        if let Some(idx) = self.index_bytes(coll_dir) {
            if let Some(keys) = parse_vector_index(idx) {
                return keys
                    .iter()
                    .map(|k| coll_dir.join(format!("{:08x}", k)))
                    .collect();
            }
        }
        // Fallback: sorted child directories (already excludes "index" stream
        // since streams are never stored in `all_dirs`).
        self.child_dirs_sorted(coll_dir)
    }

    /// Element paths for a **strong reference set** collection directory.
    ///
    /// Set index format: `u32 count + N × (u32 local_key + 16-byte ref AUID)`.
    /// Falls back to sorted children if index is missing/malformed.
    pub fn set_elements(&self, coll_dir: &Path) -> Vec<PathBuf> {
        const SET_ENTRY_SIZE: usize = 20; // 4 (local key) + 16 (AUID)

        if let Some(idx) = self.index_bytes(coll_dir) {
            if idx.len() >= 4 {
                let count = u32::from_le_bytes([idx[0], idx[1], idx[2], idx[3]]) as usize;
                let expected_len = 4 + count * SET_ENTRY_SIZE;
                if idx.len() >= expected_len {
                    let mut paths = Vec::with_capacity(count);
                    for i in 0..count {
                        let off = 4 + i * SET_ENTRY_SIZE;
                        let key = u32::from_le_bytes([
                            idx[off],
                            idx[off + 1],
                            idx[off + 2],
                            idx[off + 3],
                        ]);
                        paths.push(coll_dir.join(format!("{:08x}", key)));
                    }
                    return paths;
                }
            }
        }
        self.child_dirs_sorted(coll_dir)
    }
}

/// Decode the `"index"` stream for a vector: `[count: u32] [key: u32] × N`.
fn parse_vector_index(data: &[u8]) -> Option<Vec<u32>> {
    if data.len() < 4 {
        return None;
    }
    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if data.len() < 4 + count * 4 {
        return None;
    }
    let keys = (0..count)
        .map(|i| {
            let off = 4 + i * 4;
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
        })
        .collect();
    Some(keys)
}

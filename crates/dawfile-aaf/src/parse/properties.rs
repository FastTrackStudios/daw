//! AAF property stream decoder.
//!
//! Every AAF object stores its property values in a CFB stream named
//! `"properties"`. This module parses that stream into a typed [`Properties`]
//! map keyed by PID.
//!
//! # Property stream layout
//!
//! ```text
//! [0..2]   u16 LE  byte-order mark  (0xFFFE = little-endian)
//! [2..4]   u16 LE  version          (expect 0x0001)
//! [4..]    entries (repeated until end of stream):
//!   [+0..2]  u16 LE  PID
//!   [+2..4]  u16 LE  stored form
//!   [+4..8]  u32 LE  byte count (length of value data that follows)
//!   [+8..]   bytes[byte_count]  value
//! ```
//!
//! Stored form codes (relevant subset):
//! - `0x0002` — SF_DATA: raw typed bytes (int, string, AUID, …)
//! - `0x0082` — SF_DATA_STREAM: CFB stream; value = stream name (ASCII)
//! - `0x0022` — SF_STRONG_OBJECT_REFERENCE: value = child dir name (ASCII, null-terminated)
//! - `0x0023` — SF_STRONG_OBJECT_REFERENCE_VECTOR: value = 4+4+name header
//! - `0x0024` — SF_STRONG_OBJECT_REFERENCE_SET: value = 4+4+name header
//! - `0x0032` — SF_WEAK_OBJECT_REFERENCE: value = target AUID (16 bytes)
//! - `0x0033` — SF_WEAK_OBJECT_REFERENCE_VECTOR
//! - `0x0034` — SF_WEAK_OBJECT_REFERENCE_SET

use crate::error::{AafError, AafResult};
use crate::parse::auid::{Auid, MobId};
use crate::types::EditRate;
use std::path::Path;

// ─── StoredForm ──────────────────────────────────────────────────────────────

/// How a property value is stored in the `properties` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredForm {
    Data,
    DataStream,
    StrongRef,
    StrongRefVector,
    StrongRefSet,
    WeakRef,
    WeakRefVector,
    WeakRefSet,
    Unknown(u16),
}

impl From<u16> for StoredForm {
    fn from(v: u16) -> Self {
        match v {
            0x0002 => Self::Data,
            0x0082 => Self::DataStream,
            0x0022 => Self::StrongRef,
            0x0023 => Self::StrongRefVector,
            0x0024 => Self::StrongRefSet,
            0x0032 => Self::WeakRef,
            0x0033 => Self::WeakRefVector,
            0x0034 => Self::WeakRefSet,
            other => Self::Unknown(other),
        }
    }
}

// ─── PropertyEntry ───────────────────────────────────────────────────────────

/// A single property decoded from the stream.
#[derive(Debug)]
pub struct PropertyEntry {
    /// Property identifier.
    pub pid: u16,
    /// How the value is stored.
    pub stored_form: StoredForm,
    /// Raw value bytes (owned; length given by the stream's byte_count field).
    pub value: Vec<u8>,
}

// ─── Properties ──────────────────────────────────────────────────────────────

/// Decoded set of properties for a single AAF object.
#[derive(Debug)]
pub struct Properties {
    entries: Vec<PropertyEntry>,
}

impl Properties {
    // ─── Parse ───────────────────────────────────────────────────────────────

    /// Decode the raw `properties` stream bytes for the object at `dir_path`.
    pub fn parse(data: &[u8], dir_path: &Path) -> AafResult<Self> {
        if data.len() < 4 {
            return Err(AafError::TruncatedStream {
                offset: 0,
                path: dir_path.to_path_buf(),
            });
        }

        let bom = u16::from_le_bytes([data[0], data[1]]);
        if bom != 0xFFFE {
            // 0xFEFF would be big-endian AAF — extremely rare (legacy SGI/IRIX).
            // We don't support it; return a clear error rather than silently
            // misparses.
            return Err(AafError::UnsupportedByteOrder {
                bom,
                path: dir_path.to_path_buf(),
            });
        }
        // version at [2..4] — we don't enforce a specific version.

        let mut entries = Vec::new();
        let mut pos = 4usize;

        while pos + 8 <= data.len() {
            let pid = u16::from_le_bytes([data[pos], data[pos + 1]]);
            let sf = StoredForm::from(u16::from_le_bytes([data[pos + 2], data[pos + 3]]));
            let byte_count =
                u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;
            pos += 8;

            if pos + byte_count > data.len() {
                return Err(AafError::TruncatedStream {
                    offset: pos,
                    path: dir_path.to_path_buf(),
                });
            }

            let value = data[pos..pos + byte_count].to_vec();
            pos += byte_count;

            entries.push(PropertyEntry {
                pid,
                stored_form: sf,
                value,
            });
        }

        Ok(Self { entries })
    }

    // ─── Raw access ──────────────────────────────────────────────────────────

    /// Find the first entry with the given PID.
    pub fn get(&self, pid: u16) -> Option<&PropertyEntry> {
        self.entries.iter().find(|e| e.pid == pid)
    }

    /// Iterate over all property entries.
    pub fn iter(&self) -> impl Iterator<Item = &PropertyEntry> {
        self.entries.iter()
    }

    // ─── Class AUID ──────────────────────────────────────────────────────────

    /// The class AUID of this object (`PID_OBJ_CLASS = 0x0101`).
    pub fn class_auid(&self) -> Option<Auid> {
        self.auid(super::pids::PID_OBJ_CLASS)
    }

    // ─── Scalar accessors ────────────────────────────────────────────────────

    pub fn u8(&self, pid: u16) -> Option<u8> {
        let v = &self.get(pid)?.value;
        if v.len() >= 1 { Some(v[0]) } else { None }
    }

    pub fn u16_le(&self, pid: u16) -> Option<u16> {
        let v = &self.get(pid)?.value;
        if v.len() >= 2 {
            Some(u16::from_le_bytes([v[0], v[1]]))
        } else {
            None
        }
    }

    pub fn i32_le(&self, pid: u16) -> Option<i32> {
        let v = &self.get(pid)?.value;
        if v.len() >= 4 {
            Some(i32::from_le_bytes([v[0], v[1], v[2], v[3]]))
        } else {
            None
        }
    }

    pub fn u32_le(&self, pid: u16) -> Option<u32> {
        let v = &self.get(pid)?.value;
        if v.len() >= 4 {
            Some(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
        } else {
            None
        }
    }

    pub fn i64_le(&self, pid: u16) -> Option<i64> {
        let v = &self.get(pid)?.value;
        if v.len() >= 8 {
            Some(i64::from_le_bytes([
                v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7],
            ]))
        } else {
            None
        }
    }

    pub fn u64_le(&self, pid: u16) -> Option<u64> {
        let v = &self.get(pid)?.value;
        if v.len() >= 8 {
            Some(u64::from_le_bytes([
                v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7],
            ]))
        } else {
            None
        }
    }

    // ─── Compound accessors ──────────────────────────────────────────────────

    /// Read a 16-byte AUID from a `SF_DATA` property.
    ///
    /// Also accepts `SF_WEAK_OBJECT_REFERENCE` since many DataDefinition and
    /// similar weak refs store just the 16-byte AUID in the value field.
    pub fn auid(&self, pid: u16) -> Option<Auid> {
        let entry = self.get(pid)?;
        Auid::from_bytes(&entry.value)
    }

    /// Read a 32-byte MobID from a `SF_DATA` property.
    pub fn mob_id(&self, pid: u16) -> Option<MobId> {
        let entry = self.get(pid)?;
        MobId::from_bytes(&entry.value)
    }

    /// Read a rational edit rate: 8 bytes = i32 numerator + i32 denominator (LE).
    pub fn edit_rate(&self, pid: u16) -> Option<EditRate> {
        let v = &self.get(pid)?.value;
        if v.len() >= 8 {
            let num = i32::from_le_bytes([v[0], v[1], v[2], v[3]]);
            let den = i32::from_le_bytes([v[4], v[5], v[6], v[7]]);
            if den != 0 {
                Some(EditRate {
                    numerator: num,
                    denominator: den,
                })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Decode a UTF-16LE string property, stripping any null terminator.
    pub fn string(&self, pid: u16) -> Option<String> {
        let v = &self.get(pid)?.value;
        if v.len() < 2 || v.len() % 2 != 0 {
            return None;
        }
        let words: Vec<u16> = v
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        // Strip null terminator if present.
        let end = words.iter().position(|&w| w == 0).unwrap_or(words.len());
        String::from_utf16(&words[..end]).ok()
    }

    // ─── Strong reference accessors ──────────────────────────────────────────

    /// Extract the child-directory name from a `SF_STRONG_OBJECT_REFERENCE`
    /// property value (null-terminated ASCII/Latin-1).
    pub fn strong_ref_name(&self, pid: u16) -> Option<String> {
        let entry = self.get(pid)?;
        if entry.stored_form != StoredForm::StrongRef {
            return None;
        }
        let null = entry.value.iter().position(|&b| b == 0)?;
        String::from_utf8(entry.value[..null].to_vec()).ok()
    }

    /// Extract the `(count, collection_dir_name)` from a
    /// `SF_STRONG_OBJECT_REFERENCE_VECTOR` or `SF_STRONG_OBJECT_REFERENCE_SET`
    /// property value.
    ///
    /// The layout is:
    /// ```text
    /// [0..4]  u32 LE  count
    /// [4..8]  u32 LE  first_free_key  (used for writing; ignored here)
    /// [8..]   null-terminated ASCII   collection directory name
    /// ```
    pub fn strong_ref_collection(&self, pid: u16) -> Option<(u32, String)> {
        let entry = self.get(pid)?;
        if !matches!(
            entry.stored_form,
            StoredForm::StrongRefVector | StoredForm::StrongRefSet
        ) {
            return None;
        }
        let v = &entry.value;
        if v.len() < 9 {
            return None; // 4 + 4 + at least one byte of name
        }
        let count = u32::from_le_bytes([v[0], v[1], v[2], v[3]]);
        let null = v[8..].iter().position(|&b| b == 0)?;
        let name = String::from_utf8(v[8..8 + null].to_vec()).ok()?;
        Some((count, name))
    }
}

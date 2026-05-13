//! Codec primitives — wire-field ↔ `LoroMap` value reads and writes.
//!
//! The shape every `EntityCrdt::encode_into` / `decode_from` /
//! `apply_update` body uses. Hand-written today; the architect derive
//! macro will emit these calls from field attributes in a future
//! revision, at which point this module becomes the runtime support
//! crate the emitted code links against.
//!
//! ## Storage choices
//!
//! - **Uuid** → RFC4122 string. Round-trips losslessly, sorts
//!   lexicographically (which is *not* timestamp order — use a
//!   dedicated `created_at` field for that).
//! - **DateTime\<Utc\>** → RFC3339 string. Sub-second precision is
//!   preserved at the nanosecond level chrono emits; round-tripping
//!   through parse can normalize nanos → micros on some platforms.
//! - **u32 / i64** → `LoroValue::I64`. `u32` is range-checked on read.
//! - **bool** → `LoroValue::Bool`.
//! - **Option\<T\>** → omit the key when `None` (also accept
//!   `LoroValue::Null` on read for forward-compat).
//! - **Vec\<String\>** → tab-separated single string. Naive LWW on
//!   the whole vec; fine for low-conflict cases (tags). Promote to
//!   a `LoroList` sub-container when concurrent edits become a hot
//!   path — the codec call sites stay the same, the codec internals
//!   change.

use architect::RepoError;
use chrono::{DateTime, Utc};
use loro::{LoroMap, LoroValue};
use uuid::Uuid;

/// Wrap any Loro error into the architect `RepoError` shape so call
/// sites stay quiet. Used by every write primitive below.
pub fn loro_err<E: std::fmt::Display>(e: E) -> RepoError {
    RepoError::Internal(format!("loro: {e}"))
}

// ── Writes ────────────────────────────────────────────────────────────

pub fn write_str(m: &LoroMap, k: &str, v: &str) -> Result<(), RepoError> {
    m.insert(k, v).map_err(loro_err)
}

pub fn write_opt_str(m: &LoroMap, k: &str, v: Option<&str>) -> Result<(), RepoError> {
    match v {
        Some(s) => write_str(m, k, s),
        None => {
            let _ = m.delete(k);
            Ok(())
        }
    }
}

pub fn write_uuid(m: &LoroMap, k: &str, v: Uuid) -> Result<(), RepoError> {
    write_str(m, k, &v.to_string())
}

pub fn write_opt_uuid(m: &LoroMap, k: &str, v: Option<Uuid>) -> Result<(), RepoError> {
    match v {
        Some(u) => write_uuid(m, k, u),
        None => {
            let _ = m.delete(k);
            Ok(())
        }
    }
}

pub fn write_dt(m: &LoroMap, k: &str, v: DateTime<Utc>) -> Result<(), RepoError> {
    write_str(m, k, &v.to_rfc3339())
}

pub fn write_opt_dt(m: &LoroMap, k: &str, v: Option<DateTime<Utc>>) -> Result<(), RepoError> {
    match v {
        Some(dt) => write_dt(m, k, dt),
        None => {
            let _ = m.delete(k);
            Ok(())
        }
    }
}

pub fn write_bool(m: &LoroMap, k: &str, v: bool) -> Result<(), RepoError> {
    m.insert(k, v).map_err(loro_err)
}

pub fn write_opt_bool(m: &LoroMap, k: &str, v: Option<bool>) -> Result<(), RepoError> {
    match v {
        Some(b) => write_bool(m, k, b),
        None => {
            let _ = m.delete(k);
            Ok(())
        }
    }
}

pub fn write_i64(m: &LoroMap, k: &str, v: i64) -> Result<(), RepoError> {
    m.insert(k, v).map_err(loro_err)
}

pub fn write_opt_i64(m: &LoroMap, k: &str, v: Option<i64>) -> Result<(), RepoError> {
    match v {
        Some(n) => write_i64(m, k, n),
        None => {
            let _ = m.delete(k);
            Ok(())
        }
    }
}

pub fn write_u32(m: &LoroMap, k: &str, v: u32) -> Result<(), RepoError> {
    write_i64(m, k, v as i64)
}

pub fn write_opt_u32(m: &LoroMap, k: &str, v: Option<u32>) -> Result<(), RepoError> {
    match v {
        Some(n) => write_u32(m, k, n),
        None => {
            let _ = m.delete(k);
            Ok(())
        }
    }
}

/// Tab-separated encoding. Naive LWW on the whole vec — fine for
/// tags and other rarely-conflicting lists. Upgrade individual call
/// sites to `LoroList` sub-containers when concurrent edits matter.
pub fn write_string_list(m: &LoroMap, k: &str, v: &[String]) -> Result<(), RepoError> {
    let joined = v.join("\t");
    write_str(m, k, &joined)
}

pub fn write_opt_string_list(m: &LoroMap, k: &str, v: Option<&[String]>) -> Result<(), RepoError> {
    match v {
        Some(slice) => write_string_list(m, k, slice),
        None => {
            let _ = m.delete(k);
            Ok(())
        }
    }
}

// ── Reads ─────────────────────────────────────────────────────────────

pub fn read_str(m: &LoroMap, k: &str) -> Result<String, RepoError> {
    match m.get(k) {
        Some(loro::ValueOrContainer::Value(LoroValue::String(s))) => Ok((*s).to_string()),
        Some(other) => Err(RepoError::Internal(format!(
            "expected string at `{k}`, got {other:?}"
        ))),
        None => Err(RepoError::Internal(format!("missing key `{k}`"))),
    }
}

pub fn read_opt_str(m: &LoroMap, k: &str) -> Result<Option<String>, RepoError> {
    match m.get(k) {
        None => Ok(None),
        Some(loro::ValueOrContainer::Value(LoroValue::Null)) => Ok(None),
        Some(loro::ValueOrContainer::Value(LoroValue::String(s))) => Ok(Some((*s).to_string())),
        Some(other) => Err(RepoError::Internal(format!(
            "expected string at `{k}`, got {other:?}"
        ))),
    }
}

pub fn read_uuid(m: &LoroMap, k: &str) -> Result<Uuid, RepoError> {
    Uuid::parse_str(&read_str(m, k)?)
        .map_err(|e| RepoError::Internal(format!("bad uuid at `{k}`: {e}")))
}

pub fn read_opt_uuid(m: &LoroMap, k: &str) -> Result<Option<Uuid>, RepoError> {
    match read_opt_str(m, k)? {
        None => Ok(None),
        Some(s) => Uuid::parse_str(&s)
            .map(Some)
            .map_err(|e| RepoError::Internal(format!("bad uuid at `{k}`: {e}"))),
    }
}

pub fn read_dt(m: &LoroMap, k: &str) -> Result<DateTime<Utc>, RepoError> {
    parse_dt(&read_str(m, k)?, k)
}

pub fn read_opt_dt(m: &LoroMap, k: &str) -> Result<Option<DateTime<Utc>>, RepoError> {
    match read_opt_str(m, k)? {
        None => Ok(None),
        Some(s) => parse_dt(&s, k).map(Some),
    }
}

fn parse_dt(s: &str, k: &str) -> Result<DateTime<Utc>, RepoError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| RepoError::Internal(format!("bad timestamp at `{k}`: {e}")))
}

pub fn read_bool(m: &LoroMap, k: &str) -> Result<bool, RepoError> {
    match m.get(k) {
        Some(loro::ValueOrContainer::Value(LoroValue::Bool(b))) => Ok(b),
        Some(other) => Err(RepoError::Internal(format!(
            "expected bool at `{k}`, got {other:?}"
        ))),
        None => Err(RepoError::Internal(format!("missing key `{k}`"))),
    }
}

pub fn read_opt_bool(m: &LoroMap, k: &str) -> Result<Option<bool>, RepoError> {
    match m.get(k) {
        None => Ok(None),
        Some(loro::ValueOrContainer::Value(LoroValue::Null)) => Ok(None),
        Some(loro::ValueOrContainer::Value(LoroValue::Bool(b))) => Ok(Some(b)),
        Some(other) => Err(RepoError::Internal(format!(
            "expected bool at `{k}`, got {other:?}"
        ))),
    }
}

pub fn read_i64(m: &LoroMap, k: &str) -> Result<i64, RepoError> {
    match m.get(k) {
        Some(loro::ValueOrContainer::Value(LoroValue::I64(n))) => Ok(n),
        Some(other) => Err(RepoError::Internal(format!(
            "expected i64 at `{k}`, got {other:?}"
        ))),
        None => Err(RepoError::Internal(format!("missing key `{k}`"))),
    }
}

pub fn read_opt_i64(m: &LoroMap, k: &str) -> Result<Option<i64>, RepoError> {
    match m.get(k) {
        None => Ok(None),
        Some(loro::ValueOrContainer::Value(LoroValue::Null)) => Ok(None),
        Some(loro::ValueOrContainer::Value(LoroValue::I64(n))) => Ok(Some(n)),
        Some(other) => Err(RepoError::Internal(format!(
            "expected i64 at `{k}`, got {other:?}"
        ))),
    }
}

pub fn read_u32(m: &LoroMap, k: &str) -> Result<u32, RepoError> {
    let n = read_i64(m, k)?;
    if n < 0 || n > u32::MAX as i64 {
        return Err(RepoError::Internal(format!(
            "out of range u32 at `{k}`: {n}"
        )));
    }
    Ok(n as u32)
}

pub fn read_opt_u32(m: &LoroMap, k: &str) -> Result<Option<u32>, RepoError> {
    match read_opt_i64(m, k)? {
        None => Ok(None),
        Some(n) => {
            if n < 0 || n > u32::MAX as i64 {
                return Err(RepoError::Internal(format!(
                    "out of range u32 at `{k}`: {n}"
                )));
            }
            Ok(Some(n as u32))
        }
    }
}

pub fn read_string_list(m: &LoroMap, k: &str) -> Result<Vec<String>, RepoError> {
    let raw = read_str(m, k)?;
    if raw.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(raw.split('\t').map(str::to_string).collect())
    }
}

pub fn read_opt_string_list(m: &LoroMap, k: &str) -> Result<Option<Vec<String>>, RepoError> {
    match read_opt_str(m, k)? {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(Some(Vec::new())),
        Some(s) => Ok(Some(s.split('\t').map(str::to_string).collect())),
    }
}

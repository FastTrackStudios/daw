//! Bytes across the boundary: `clap_ostream` both ways, and C strings.

use std::ffi::{CStr, CString, c_char, c_void};

use clap_sys::stream::clap_ostream;

/// An output stream into a `Vec<u8>`: what a caller hands the other side.
pub struct VecOstream {
    raw: clap_ostream,
    buf: Box<Vec<u8>>,
}

unsafe extern "C" fn vec_write(
    stream: *const clap_ostream,
    buffer: *const c_void,
    size: u64,
) -> i64 {
    if stream.is_null() || (buffer.is_null() && size > 0) {
        return -1;
    }
    // SAFETY: `ctx` is the `Vec<u8>` of the `VecOstream` that owns `stream`.
    let vec = unsafe { &mut *((*stream).ctx.cast::<Vec<u8>>()) };
    let Ok(n) = usize::try_from(size) else {
        return -1;
    };
    if n > 0 {
        // SAFETY: the writer promises `size` readable bytes at `buffer`.
        vec.extend_from_slice(unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), n) });
    }
    i64::try_from(n).unwrap_or(-1)
}

impl Default for VecOstream {
    fn default() -> Self {
        Self::new()
    }
}

impl VecOstream {
    #[must_use]
    pub fn new() -> Self {
        let mut buf = Box::new(Vec::new());
        let ctx = std::ptr::from_mut::<Vec<u8>>(&mut buf).cast::<c_void>();
        Self {
            raw: clap_ostream {
                ctx,
                write: Some(vec_write),
            },
            buf,
        }
    }

    /// The stream to hand over; valid while `self` lives.
    #[must_use]
    pub fn as_raw(&self) -> *const clap_ostream {
        &raw const self.raw
    }

    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        *self.buf
    }

    /// The bytes as UTF-8 (lossy).
    #[must_use]
    pub fn into_string(self) -> String {
        String::from_utf8_lossy(&self.buf).into_owned()
    }
}

/// Write all of `bytes` to `stream`.
///
/// # Safety
///
/// `stream` must be a valid `clap_ostream` for the duration of the call.
pub unsafe fn write_all(stream: *const clap_ostream, mut bytes: &[u8]) -> bool {
    if stream.is_null() {
        return false;
    }
    // SAFETY: the caller's contract.
    let Some(write) = (unsafe { (*stream).write }) else {
        return false;
    };
    while !bytes.is_empty() {
        let Ok(len) = u64::try_from(bytes.len()) else {
            return false;
        };
        // SAFETY: `bytes` is readable for `len` bytes.
        let n = unsafe { write(stream, bytes.as_ptr().cast(), len) };
        let Ok(n) = usize::try_from(n) else {
            return false;
        };
        if n == 0 || n > bytes.len() {
            return false;
        }
        bytes = bytes.get(n..).unwrap_or_default();
    }
    true
}

/// A C string argument as `&str`; `None` for NULL or non-UTF-8.
///
/// # Safety
///
/// `s` must be NULL or a valid NUL-terminated string for `'a`.
pub unsafe fn str_arg<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    // SAFETY: the caller's contract.
    unsafe { CStr::from_ptr(s) }.to_str().ok()
}

/// `s` as a C string, with any interior NUL dropped.
#[must_use]
pub fn c_string(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// Copy `s` into a fixed C buffer, NUL-terminated, truncated on a UTF-8
/// boundary.
pub fn copy_to_buf(s: &str, out: &mut [c_char]) {
    let Some(max) = out.len().checked_sub(1) else {
        return;
    };
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    for (dst, src) in out
        .iter_mut()
        .zip(s.as_bytes().get(..end).unwrap_or_default())
    {
        *dst = *src as c_char;
    }
    if let Some(nul) = out.get_mut(end) {
        *nul = 0;
    }
}

/// A fixed C buffer as a `String` (up to its NUL).
#[must_use]
pub fn from_buf(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

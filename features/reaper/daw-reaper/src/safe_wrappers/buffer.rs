//! Safe helpers for REAPER APIs that write into caller-provided byte buffers.

/// Call an FFI function that writes a NUL-terminated string into a fixed-size
/// buffer. Returns `None` if the callback returns `false` (indicating failure).
///
/// The callback receives `(buf_ptr: *mut i8, buf_len: i32)` and should return
/// `true` on success.
pub fn with_string_buffer<F>(size: usize, f: F) -> Option<String>
where
    F: FnOnce(*mut i8, i32) -> bool,
{
    let mut buf = vec![0u8; size];
    if !f(buf.as_mut_ptr() as *mut i8, size as i32) {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..len]).into_owned())
}

/// Like [`with_string_buffer`] but uses the integer return value as the
/// success indicator (> 0 means success).
pub fn with_string_buffer_i32<F>(size: usize, f: F) -> Option<String>
where
    F: FnOnce(*mut i8, i32) -> i32,
{
    let mut buf = vec![0u8; size];
    let result = f(buf.as_mut_ptr() as *mut i8, size as i32);
    if result <= 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..len]).into_owned())
}

/// Read a NUL-terminated string from a raw byte buffer (e.g. stack-allocated).
pub fn string_from_buffer(buf: &[u8]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

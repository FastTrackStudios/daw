//! A file that arrives in pieces: the bytes of a proxy fetched from
//! somewhere else (Task, a peer), readable where they have landed.
//!
//! [`SparseBytes`] holds what has arrived — in memory, or in a file on disk
//! so a long set does not sit in RAM — and which ranges those are. A reader
//! over it ([`SparseView`]) reads what is there and, where it is not, says
//! **would block** and records the range it wanted: the decoder stops, the
//! track is silent there for now, and the fetcher knows what to bring next.
//! Nothing ever waits on the network.
//!
//! A view is the stream's header pages followed by the file from a given
//! page on — what [`crate::ogg_stream::OggStream::open_view`] decodes, so a
//! seek into a remote proxy needs only the bytes from the page before the
//! frame asked for (found by [`crate::ogg_index::OggIndex`]), never a
//! bisection across the whole file.

use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::sync::{Arc, Mutex};

/// Where the arrived bytes are kept.
enum Store {
    Memory(Vec<u8>),
    #[cfg(not(target_arch = "wasm32"))]
    File(std::fs::File),
}

struct Inner {
    store: Store,
    /// The ranges that have arrived: sorted, disjoint, not touching.
    have: Vec<Range<u64>>,
    /// The last range a reader asked for and found missing.
    wanted: Option<Range<u64>>,
}

/// A file's bytes, as they arrive.
pub struct SparseBytes {
    len: u64,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for SparseBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SparseBytes").field("len", &self.len).field("have", &self.have()).finish()
    }
}

fn lock(m: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl SparseBytes {
    /// A `len`-byte file kept in memory.
    #[must_use]
    pub fn in_memory(len: u64) -> Arc<Self> {
        let size = usize::try_from(len).unwrap_or(usize::MAX);
        Arc::new(Self {
            len,
            inner: Mutex::new(Inner { store: Store::Memory(vec![0; size]), have: Vec::new(), wanted: None }),
        })
    }

    /// A `len`-byte file kept in a file at `path` (created, sized; its
    /// contents are what has arrived).
    ///
    /// # Errors
    ///
    /// The file cannot be created.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_disk(len: u64, path: &std::path::Path) -> std::io::Result<Arc<Self>> {
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(path)?;
        file.set_len(len)?;
        Ok(Arc::new(Self {
            len,
            inner: Mutex::new(Inner { store: Store::File(file), have: Vec::new(), wanted: None }),
        }))
    }

    /// The whole file's length.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether the file is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bytes that arrived at `offset`.
    ///
    /// # Errors
    ///
    /// Writing to the on-disk store failed.
    pub fn insert(&self, offset: u64, data: &[u8]) -> std::io::Result<()> {
        let end = offset.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX)).min(self.len);
        if end <= offset {
            return Ok(());
        }
        let take = usize::try_from(end - offset).unwrap_or(0);
        let mut inner = lock(&self.inner);
        match &mut inner.store {
            Store::Memory(buf) => {
                let at = usize::try_from(offset).unwrap_or(usize::MAX);
                if let (Some(dst), Some(src)) = (buf.get_mut(at..at.saturating_add(take)), data.get(..take)) {
                    dst.copy_from_slice(src);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Store::File(file) => {
                file.seek(SeekFrom::Start(offset))?;
                std::io::Write::write_all(file, data.get(..take).unwrap_or_default())?;
            }
        }
        add_range(&mut inner.have, offset..end);
        if inner.wanted.as_ref().is_some_and(|w| covered(&inner.have, w)) {
            inner.wanted = None;
        }
        Ok(())
    }

    /// The ranges that have arrived.
    #[must_use]
    pub fn have(&self) -> Vec<Range<u64>> {
        lock(&self.inner).have.clone()
    }

    /// Whether all of `range` has arrived.
    #[must_use]
    pub fn has(&self, range: &Range<u64>) -> bool {
        covered(&lock(&self.inner).have, &(range.start..range.end.min(self.len)))
    }

    /// What of `range` has not arrived yet.
    #[must_use]
    pub fn missing(&self, range: &Range<u64>) -> Vec<Range<u64>> {
        let range = range.start..range.end.min(self.len);
        let inner = lock(&self.inner);
        let mut out = Vec::new();
        let mut at = range.start;
        for have in inner.have.iter().filter(|h| h.end > range.start && h.start < range.end) {
            if have.start > at {
                out.push(at..have.start);
            }
            at = at.max(have.end);
        }
        if at < range.end {
            out.push(at..range.end);
        }
        out
    }

    /// The last range a reader wanted and did not find (cleared once it
    /// arrives) — what a fetcher brings next.
    #[must_use]
    pub fn wanted(&self) -> Option<Range<u64>> {
        lock(&self.inner).wanted.clone()
    }

    /// Read into `buf` from `offset`, as much as has arrived contiguously;
    /// `None` (and `wanted` set) when the first byte has not.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<Option<usize>> {
        if offset >= self.len {
            return Ok(Some(0));
        }
        let mut inner = lock(&self.inner);
        let Some(have) = inner.have.iter().find(|h| h.start <= offset && offset < h.end).cloned() else {
            let want = offset..offset.saturating_add(u64::try_from(buf.len()).unwrap_or(0)).min(self.len);
            inner.wanted = Some(want);
            return Ok(None);
        };
        let n = usize::try_from((have.end - offset).min(u64::try_from(buf.len()).unwrap_or(u64::MAX))).unwrap_or(0);
        match &mut inner.store {
            Store::Memory(data) => {
                let at = usize::try_from(offset).unwrap_or(0);
                if let (Some(src), Some(dst)) = (data.get(at..at.saturating_add(n)), buf.get_mut(..n)) {
                    dst.copy_from_slice(src);
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Store::File(file) => {
                file.seek(SeekFrom::Start(offset))?;
                file.read_exact(buf.get_mut(..n).unwrap_or_default())?;
            }
        }
        Ok(Some(n))
    }
}

fn covered(have: &[Range<u64>], range: &Range<u64>) -> bool {
    range.is_empty() || have.iter().any(|h| h.start <= range.start && range.end <= h.end)
}

/// Add `r` to a sorted, disjoint set, merging what it touches.
fn add_range(set: &mut Vec<Range<u64>>, r: Range<u64>) {
    let mut merged = r;
    set.retain(|h| {
        if h.end < merged.start || h.start > merged.end {
            true
        } else {
            merged = merged.start.min(h.start)..merged.end.max(h.end);
            false
        }
    });
    let at = set.partition_point(|h| h.start < merged.start);
    set.insert(at, merged);
}

/// A reader over a stream's header pages followed by the file from
/// `from` on — positions `0..header.len()` are the header, then the file.
pub struct SparseView {
    bytes: Arc<SparseBytes>,
    header: Range<u64>,
    from: u64,
    pos: u64,
}

impl SparseView {
    /// The header (`header`, bytes of the file) then the file from `from`.
    #[must_use]
    pub fn new(bytes: Arc<SparseBytes>, header: Range<u64>, from: u64) -> Self {
        Self { bytes, header, from, pos: 0 }
    }

    /// The whole file, from its start.
    #[must_use]
    pub fn whole(bytes: Arc<SparseBytes>) -> Self {
        Self { bytes, header: 0..0, from: 0, pos: 0 }
    }

    fn header_len(&self) -> u64 {
        self.header.end - self.header.start
    }

    fn view_len(&self) -> u64 {
        self.header_len() + self.bytes.len().saturating_sub(self.from)
    }

    /// Where view position `pos` is in the file.
    fn file_offset(&self, pos: u64) -> u64 {
        let h = self.header_len();
        if pos < h { self.header.start + pos } else { self.from + (pos - h) }
    }
}

impl Read for SparseView {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let h = self.header_len();
        // Never read across the header's end in one go: the file continues
        // elsewhere after it.
        let room = if self.pos < h { h - self.pos } else { u64::MAX };
        let n = usize::try_from(room).unwrap_or(usize::MAX).min(buf.len());
        let offset = self.file_offset(self.pos);
        match self.bytes.read_at(offset, buf.get_mut(..n).unwrap_or_default())? {
            Some(got) => {
                self.pos += u64::try_from(got).unwrap_or(0);
                Ok(got)
            }
            None => Err(std::io::Error::new(std::io::ErrorKind::WouldBlock, "not arrived yet")),
        }
    }
}

impl Seek for SparseView {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let len = i128::from(self.view_len());
        let at = match to {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(d) => len + i128::from(d),
            SeekFrom::Current(d) => i128::from(self.pos) + i128::from(d),
        };
        self.pos = u64::try_from(at.clamp(0, len)).unwrap_or(0);
        Ok(self.pos)
    }
}

impl symphonia_core::io::MediaSource for SparseView {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.view_len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_has_arrived_reads_and_what_has_not_would_block() {
        let bytes = SparseBytes::in_memory(100);
        bytes.insert(10, &[7; 20]).unwrap();
        let mut view = SparseView::whole(Arc::clone(&bytes));
        view.seek(SeekFrom::Start(12)).unwrap();
        let mut buf = [0u8; 50];
        assert_eq!(view.read(&mut buf).unwrap(), 18, "up to the end of what arrived");
        let e = view.read(&mut buf).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(bytes.wanted(), Some(30..80));
        bytes.insert(30, &[9; 50]).unwrap();
        assert_eq!(bytes.wanted(), None, "cleared once it arrives");
        assert_eq!(view.read(&mut buf).unwrap(), 50);
        assert_eq!(bytes.have(), vec![10..80]);
        assert_eq!(bytes.missing(&(0..100)), vec![0..10, 80..100]);
    }

    #[test]
    fn a_view_is_the_header_then_the_file_from_a_page() {
        let bytes = SparseBytes::in_memory(10);
        bytes.insert(0, b"HHabcdefgh").unwrap();
        let mut view = SparseView::new(bytes, 0..2, 6);
        let mut out = Vec::new();
        view.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"HHefgh");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn on_disk_holds_what_arrived() {
        let path = std::env::temp_dir().join(format!("fts-sparse-{}.bin", std::process::id()));
        let bytes = SparseBytes::on_disk(64, &path).unwrap();
        bytes.insert(32, &[5; 16]).unwrap();
        let mut view = SparseView::whole(Arc::clone(&bytes));
        view.seek(SeekFrom::Start(40)).unwrap();
        let mut buf = [0u8; 4];
        view.read_exact(&mut buf).unwrap();
        assert_eq!(buf, [5; 4]);
        let _ = std::fs::remove_file(path);
    }
}

//! A SQLite file's tables, read from its bytes: enough of the file format
//! (<https://www.sqlite.org/fileformat2.html>) to list every row of a table
//! — the schema on page 1, table b-trees (interior and leaf pages), records
//! and the overflow chains a large value spills into. Read-only, no
//! indexes, no write-ahead log (a file's `-wal` is whoever had it open,
//! not part of the copy a browser is handed).

/// A column's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    /// The value as text, if it is text.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The value as an integer, if it is one.
    #[must_use]
    pub fn int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// The value's bytes, if it is a blob (or text, read as its bytes).
    #[must_use]
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Blob(bytes) => Some(bytes),
            Self::Text(text) => Some(text.into_bytes()),
            _ => None,
        }
    }
}

/// One row of a table: its rowid (which an `INTEGER PRIMARY KEY` column
/// is, stored as NULL in the record) and its columns in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub rowid: i64,
    pub values: Vec<Value>,
}

impl Row {
    /// Column `index`, `Null` past the record's end (a column added after
    /// the row was written).
    #[must_use]
    pub fn get(&self, index: usize) -> &Value {
        self.values.get(index).unwrap_or(&Value::Null)
    }

    /// Column `index`, taken.
    pub fn take(&mut self, index: usize) -> Value {
        self.values
            .get_mut(index)
            .map_or(Value::Null, |value| std::mem::replace(value, Value::Null))
    }
}

/// Why a file could not be read.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SqliteError {
    #[error("not a SQLite file")]
    NotSqlite,
    #[error("text is not UTF-8 in this file")]
    NotUtf8,
    #[error("page {0} is not in the file")]
    NoPage(u32),
    #[error("the file has no table {0}")]
    NoTable(String),
    #[error("malformed file: {0}")]
    Malformed(&'static str),
}

type Result<T> = std::result::Result<T, SqliteError>;

/// How deep a table's b-tree may go before the file is taken as broken (a
/// cycle): far past any real one.
const MAX_DEPTH: usize = 40;

/// A SQLite file, held as its bytes.
pub struct Db<'a> {
    bytes: &'a [u8],
    page_size: usize,
    /// The page size less the bytes reserved at each page's end.
    usable: usize,
}

impl<'a> Db<'a> {
    /// The file in `bytes`, its header checked.
    ///
    /// # Errors
    /// Not a SQLite file, or one whose text is not UTF-8.
    pub fn new(bytes: &'a [u8]) -> Result<Self> {
        let header = bytes.get(..100).ok_or(SqliteError::NotSqlite)?;
        if &header[..16] != b"SQLite format 3\0" {
            return Err(SqliteError::NotSqlite);
        }
        let page_size = match u16::from_be_bytes([header[16], header[17]]) {
            1 => 65_536,
            n => usize::from(n),
        };
        if !page_size.is_power_of_two() || !(512..=65_536).contains(&page_size) {
            return Err(SqliteError::Malformed("page size"));
        }
        let usable = page_size - usize::from(header[20]);
        if usable < 480 {
            return Err(SqliteError::Malformed("reserved space"));
        }
        // 0: a file with nothing in it yet; 1: UTF-8.
        if !matches!(be_u32(&header[56..60]), 0 | 1) {
            return Err(SqliteError::NotUtf8);
        }
        Ok(Self {
            bytes,
            page_size,
            usable,
        })
    }

    /// Every row of table `name`, in rowid order.
    ///
    /// # Errors
    /// No such table, or the file is malformed.
    pub fn rows(&self, name: &str) -> Result<Vec<Row>> {
        let root = self
            .rows_from(1)?
            .into_iter()
            .find(|row| row.get(0).text() == Some("table") && row.get(1).text() == Some(name))
            .and_then(|row| row.get(3).int())
            .ok_or_else(|| SqliteError::NoTable(name.to_owned()))?;
        let root = u32::try_from(root).map_err(|_| SqliteError::Malformed("root page"))?;
        self.rows_from(root)
    }

    /// Every row of the table b-tree rooted at page `root`.
    fn rows_from(&self, root: u32) -> Result<Vec<Row>> {
        let mut rows = Vec::new();
        self.walk(root, 0, &mut rows)?;
        Ok(rows)
    }

    fn page(&self, number: u32) -> Result<&'a [u8]> {
        let index = usize::try_from(number)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .ok_or(SqliteError::NoPage(number))?;
        let start = index * self.page_size;
        self.bytes
            .get(start..start + self.page_size)
            .ok_or(SqliteError::NoPage(number))
    }

    fn walk(&self, number: u32, depth: usize, rows: &mut Vec<Row>) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(SqliteError::Malformed("b-tree too deep"));
        }
        let page = self.page(number)?;
        // Page 1 starts with the file's header.
        let at = if number == 1 { 100 } else { 0 };
        let kind = *page.get(at).ok_or(SqliteError::Malformed("page header"))?;
        let cells = usize::from(be_u16(slice(page, at + 3, 2)?));
        match kind {
            // An interior table page: each cell a child and the largest
            // rowid under it, the rightmost child after them.
            0x05 => {
                for i in 0..cells {
                    let cell = cell_offset(page, at + 12, i)?;
                    let child = be_u32(slice(page, cell, 4)?);
                    self.walk(child, depth + 1, rows)?;
                }
                self.walk(be_u32(slice(page, at + 8, 4)?), depth + 1, rows)
            }
            // A leaf: the rows.
            0x0D => {
                for i in 0..cells {
                    let cell = cell_offset(page, at + 8, i)?;
                    rows.push(self.leaf_cell(page, cell)?);
                }
                Ok(())
            }
            _ => Err(SqliteError::Malformed("not a table b-tree page")),
        }
    }

    fn leaf_cell(&self, page: &[u8], cell: usize) -> Result<Row> {
        let (size, a) = varint(page.get(cell..).unwrap_or_default())?;
        let (rowid, b) = varint(page.get(cell + a..).unwrap_or_default())?;
        let size = usize::try_from(size).map_err(|_| SqliteError::Malformed("payload size"))?;
        let start = cell + a + b;
        let local = local_size(size, self.usable);
        let mut payload = slice(page, start, local)?.to_vec();
        if local < size {
            let first = be_u32(slice(page, start + local, 4)?);
            self.overflow(first, size - local, &mut payload)?;
        }
        Ok(Row {
            rowid: rowid as i64,
            values: record(&payload)?,
        })
    }

    /// Follow an overflow chain from page `next`, `left` bytes of payload
    /// still to come.
    fn overflow(&self, mut next: u32, mut left: usize, payload: &mut Vec<u8>) -> Result<()> {
        let pages = self.bytes.len() / self.page_size;
        for _ in 0..pages {
            if left == 0 {
                return Ok(());
            }
            let page = self.page(next)?;
            let take = left.min(self.usable - 4);
            payload.extend_from_slice(slice(page, 4, take)?);
            left -= take;
            next = be_u32(&page[..4]);
        }
        if left == 0 {
            Ok(())
        } else {
            Err(SqliteError::Malformed("overflow chain"))
        }
    }
}

/// How much of a table leaf cell's `size`-byte payload is on its page,
/// the rest overflowing (the file format's rule for table b-tree leaves).
fn local_size(size: usize, usable: usize) -> usize {
    let max = usable - 35;
    if size <= max {
        return size;
    }
    let min = (usable - 12) * 32 / 255 - 23;
    let spill = min + (size - min) % (usable - 4);
    if spill <= max { spill } else { min }
}

fn cell_offset(page: &[u8], array: usize, index: usize) -> Result<usize> {
    Ok(usize::from(be_u16(slice(page, array + 2 * index, 2)?)))
}

fn slice(bytes: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    bytes
        .get(at..at + len)
        .ok_or(SqliteError::Malformed("past the page's end"))
}

fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// A SQLite varint at the start of `bytes`, and how many bytes it took:
/// up to eight of seven bits, high bit first, then a ninth of all eight.
fn varint(bytes: &[u8]) -> Result<(u64, usize)> {
    let mut value: u64 = 0;
    for (i, &byte) in bytes.iter().take(9).enumerate() {
        if i == 8 {
            return Ok(((value << 8) | u64::from(byte), 9));
        }
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err(SqliteError::Malformed("varint"))
}

/// A record's columns: a header of serial types, then their values.
fn record(payload: &[u8]) -> Result<Vec<Value>> {
    let (header, mut at) = varint(payload)?;
    let header = usize::try_from(header).map_err(|_| SqliteError::Malformed("record header"))?;
    let mut body = header;
    let mut values = Vec::new();
    while at < header {
        let (kind, used) = varint(payload.get(at..header).unwrap_or_default())?;
        at += used;
        let (value, len) = column(kind, payload.get(body..).unwrap_or_default())?;
        body += len;
        values.push(value);
    }
    Ok(values)
}

/// One column of serial type `kind` at the start of `bytes`, and its
/// length.
fn column(kind: u64, bytes: &[u8]) -> Result<(Value, usize)> {
    let int = |len: usize| -> Result<(Value, usize)> {
        let raw = slice(bytes, 0, len)?;
        // Big-endian, two's complement: sign-extended from the first byte.
        let first = if raw[0] & 0x80 == 0 { 0 } else { -1i64 };
        let value = raw.iter().fold(first, |n, &b| (n << 8) | i64::from(b));
        Ok((Value::Int(value), len))
    };
    match kind {
        0 => Ok((Value::Null, 0)),
        1 => int(1),
        2 => int(2),
        3 => int(3),
        4 => int(4),
        5 => int(6),
        6 => int(8),
        7 => {
            let raw = slice(bytes, 0, 8)?;
            let bits = raw.iter().fold(0u64, |n, &b| (n << 8) | u64::from(b));
            Ok((Value::Real(f64::from_bits(bits)), 8))
        }
        8 => Ok((Value::Int(0), 0)),
        9 => Ok((Value::Int(1), 0)),
        10 | 11 => Err(SqliteError::Malformed("reserved serial type")),
        n => {
            let len = usize::try_from((n - 12) / 2)
                .map_err(|_| SqliteError::Malformed("value length"))?;
            let raw = slice(bytes, 0, len)?.to_vec();
            if n % 2 == 0 {
                Ok((Value::Blob(raw), len))
            } else {
                let text = String::from_utf8(raw).map_err(|_| SqliteError::NotUtf8)?;
                Ok((Value::Text(text), len))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_take_seven_bits_a_byte_and_all_eight_of_the_ninth() {
        assert_eq!(varint(&[0x05]), Ok((5, 1)));
        assert_eq!(varint(&[0x81, 0x00]), Ok((128, 2)));
        assert_eq!(varint(&[0xff; 9]), Ok((u64::MAX, 9)));
        assert!(varint(&[0x80]).is_err());
    }

    #[test]
    fn integer_columns_are_sign_extended() {
        assert_eq!(column(1, &[0xff]), Ok((Value::Int(-1), 1)));
        assert_eq!(column(2, &[0x01, 0x00]), Ok((Value::Int(256), 2)));
        assert_eq!(
            column(5, &[0x80, 0, 0, 0, 0, 0]).map(|c| c.0),
            Ok(Value::Int(-(1 << 47)))
        );
        assert_eq!(column(8, &[]), Ok((Value::Int(0), 0)));
    }

    #[test]
    fn a_record_reads_text_blob_and_null() {
        // Header: 4 bytes long; text of 2, blob of 1, null.
        let payload = [4, 13 + 2 * 2, 12 + 2, 0, b'h', b'i', 0xab];
        assert_eq!(
            record(&payload),
            Ok(vec![
                Value::Text("hi".to_owned()),
                Value::Blob(vec![0xab]),
                Value::Null
            ])
        );
    }

    #[test]
    fn a_payload_past_the_page_spills_by_the_formats_rule() {
        // 4096-byte pages: up to 4061 bytes stay on the page.
        assert_eq!(local_size(4061, 4096), 4061);
        // Past that: the minimum (489) plus what does not fill whole
        // overflow pages, unless that would be more than fits.
        assert_eq!(local_size(4062, 4096), 489);
        assert_eq!(local_size(5000, 4096), 489 + (5000 - 489) % 4092);
        assert!(local_size(100_000, 4096) <= 4061);
    }

    #[test]
    fn anything_but_a_sqlite_file_is_refused() {
        assert_eq!(
            Db::new(b"this link is view-only").err(),
            Some(SqliteError::NotSqlite)
        );
    }
}

//! A small TIFF directory (IFD) reader and writer for EXIF blocks.
//!
//! An EXIF block is a TIFF structure: a header, IFD0, and sub-directories found
//! through pointer tags (EXIF 34665, GPS 34853, interoperability 40965). TIFF
//! files keep the same entries in their own IFD0, so these helpers move entries
//! between a standalone EXIF block and a TIFF file.
use crate::{Error, Result};

pub(crate) const EXIF_IFD: u16 = 34665;
pub(crate) const GPS_IFD: u16 = 34853;
pub(crate) const INTEROP_IFD: u16 = 40965;
/// IFD0 tags that describe the picture rather than the TIFF pixel layout.
/// These move between an EXIF block and a TIFF file's own IFD0.
pub(crate) const IFD0_METADATA: [u16; 11] =
    [269, 270, 271, 272, 274, 285, 305, 306, 315, 316, 33432];
const MAX_VALUE_BYTES: usize = 16 * 1024 * 1024;
/// Entries may share value bytes, so the total copied is capped separately.
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    /// The value bytes, always stored little-endian.
    pub data: Vec<u8>,
    /// The entries of the sub-directory this pointer tag leads to.
    pub sub: Option<Vec<Entry>>,
}

/// (bytes per value, bytes per byte-swapped unit) for each TIFF field type.
pub(crate) fn kind_size(kind: u16) -> Option<(usize, usize)> {
    match kind {
        1 | 2 | 6 | 7 => Some((1, 1)),
        3 | 8 => Some((2, 2)),
        4 | 9 | 11 | 13 => Some((4, 4)),
        5 | 10 => Some((8, 4)),
        12 => Some((8, 8)),
        _ => None,
    }
}

fn swap_units(data: &mut [u8], unit: usize) {
    if unit > 1 {
        for c in data.chunks_exact_mut(unit) {
            c.reverse();
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    le: bool,
    copied: std::cell::Cell<usize>,
}
impl Reader<'_> {
    fn bytes(&self, p: usize, n: usize) -> Result<&[u8]> {
        p.checked_add(n)
            .and_then(|end| self.b.get(p..end))
            .ok_or(Error::Invalid("TIFF directory points outside the data"))
    }
    fn u16(&self, p: usize) -> Result<u16> {
        let x: [u8; 2] = self.bytes(p, 2)?.try_into().unwrap();
        Ok(if self.le {
            u16::from_le_bytes(x)
        } else {
            u16::from_be_bytes(x)
        })
    }
    fn u32(&self, p: usize) -> Result<u32> {
        let x: [u8; 4] = self.bytes(p, 4)?.try_into().unwrap();
        Ok(if self.le {
            u32::from_le_bytes(x)
        } else {
            u32::from_be_bytes(x)
        })
    }
    fn ifd(&self, off: usize, depth: usize) -> Result<Vec<Entry>> {
        if depth > MAX_DEPTH {
            return Err(Error::Invalid("TIFF directories nest too deeply"));
        }
        let n = self.u16(off)? as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let p = off + 2 + i * 12;
            let tag = self.u16(p)?;
            let kind = self.u16(p + 2)?;
            let count = self.u32(p + 4)?;
            // Unknown field types cannot be sized, so they are left out.
            let Some((size, unit)) = kind_size(kind) else {
                continue;
            };
            let total = (count as usize)
                .checked_mul(size)
                .filter(|&t| t <= MAX_VALUE_BYTES)
                .ok_or(Error::Invalid("TIFF value is too large"))?;
            let at = if total <= 4 {
                p + 8
            } else {
                self.u32(p + 8)? as usize
            };
            let copied = self.copied.get() + total;
            if copied > MAX_TOTAL_BYTES {
                return Err(Error::Invalid("TIFF directories hold too much data"));
            }
            self.copied.set(copied);
            let mut data = self.bytes(at, total)?.to_vec();
            if !self.le {
                swap_units(&mut data, unit);
            }
            let sub = if matches!(tag, EXIF_IFD | GPS_IFD | INTEROP_IFD)
                && matches!(kind, 4 | 13)
                && count == 1
            {
                let target = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
                Some(self.ifd(target, depth + 1)?)
            } else if kind == 13 {
                // An offset to a directory we do not follow would go stale
                // once the block is rewritten, so the entry is left out.
                continue;
            } else {
                None
            };
            out.push(Entry {
                tag,
                kind,
                count,
                data,
                sub,
            });
        }
        Ok(out)
    }
}

/// Parse IFD0 (with its sub-directories) of a TIFF file or EXIF block.
/// A leading `Exif\0\0` marker is skipped.
pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<Entry>> {
    let b = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    let le = match b.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Err(Error::Invalid("EXIF block is not a TIFF structure")),
    };
    let r = Reader {
        b,
        le,
        copied: std::cell::Cell::new(0),
    };
    if r.u16(2)? != 42 {
        return Err(Error::Invalid("EXIF block is not a TIFF structure"));
    }
    r.ifd(r.u32(4)? as usize, 0)
}

/// Write entries as a standalone little-endian EXIF block.
///
/// The layout is fixed: IFD0 right after the header, entries sorted by tag,
/// each directory followed by its out-of-line values and then its
/// sub-directories, and values on even offsets. A block already in this
/// layout comes back byte-exact.
pub(crate) fn serialize(entries: &[Entry]) -> Result<Vec<u8>> {
    let mut out = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    write_dir(&mut out, entries)?;
    Ok(out)
}

fn offset32(v: usize) -> Result<[u8; 4]> {
    u32::try_from(v)
        .map(u32::to_le_bytes)
        .map_err(|_| Error::Invalid("EXIF block exceeds 4 GiB"))
}

fn write_dir(out: &mut Vec<u8>, entries: &[Entry]) -> Result<usize> {
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.tag);
    let n = u16::try_from(sorted.len()).map_err(|_| Error::Invalid("too many EXIF entries"))?;
    let start = out.len();
    out.extend_from_slice(&n.to_le_bytes());
    out.resize(start + 2 + 12 * sorted.len() + 4, 0);
    for (i, e) in sorted.iter().enumerate() {
        let p = start + 2 + i * 12;
        out[p..p + 2].copy_from_slice(&e.tag.to_le_bytes());
        if e.sub.is_some() {
            out[p + 2..p + 4].copy_from_slice(&4u16.to_le_bytes());
            out[p + 4..p + 8].copy_from_slice(&1u32.to_le_bytes());
            continue;
        }
        out[p + 2..p + 4].copy_from_slice(&e.kind.to_le_bytes());
        out[p + 4..p + 8].copy_from_slice(&e.count.to_le_bytes());
        if e.data.len() <= 4 {
            out[p + 8..p + 8 + e.data.len()].copy_from_slice(&e.data);
        } else {
            if out.len() % 2 == 1 {
                out.push(0);
            }
            let at = offset32(out.len())?;
            out.extend_from_slice(&e.data);
            out[p + 8..p + 12].copy_from_slice(&at);
        }
    }
    for (i, e) in sorted.iter().enumerate() {
        if let Some(sub) = &e.sub {
            let at = offset32(write_dir(out, sub)?)?;
            let p = start + 2 + i * 12;
            out[p + 8..p + 12].copy_from_slice(&at);
        }
    }
    Ok(start)
}

use crate::{Decoded, Error, Metadata, Result, checked_len};

fn u16at(b: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(p..p + 2)
            .ok_or(Error::Invalid("truncated BMP"))?
            .try_into()
            .unwrap(),
    ))
}
fn u32at(b: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4)
            .ok_or(Error::Invalid("truncated BMP"))?
            .try_into()
            .unwrap(),
    ))
}
fn component(v: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 255;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    (((v & mask) >> shift) as u64 * 255 / max as u64) as u8
}

pub(crate) fn decode(b: &[u8]) -> Result<Decoded> {
    if b.get(..2) != Some(b"BM") {
        return Err(Error::Invalid("BMP signature"));
    }
    let off = u32at(b, 10)? as usize;
    let dib = u32at(b, 14)? as usize;
    if dib < 40 || b.len() < 14 + dib {
        return Err(Error::Invalid("BMP header"));
    }
    let w = u32at(b, 18)? as i32;
    let h = u32at(b, 22)? as i32;
    if w <= 0 || h == 0 || h == i32::MIN {
        return Err(Error::Invalid("BMP dimensions"));
    }
    let width = w as u32;
    let height = h.unsigned_abs();
    let len = checked_len(width, height)?;
    if u16at(b, 26)? != 1 {
        return Err(Error::Invalid("BMP planes"));
    }
    let depth = u16at(b, 28)?;
    let compression = u32at(b, 30)?;
    if !matches!(
        (depth, compression),
        (1 | 4 | 8 | 24 | 32, 0) | (4, 2) | (8, 1) | (16 | 32, 3)
    ) {
        return Err(Error::Invalid("BMP depth/compression"));
    }
    let mut rgba8 = vec![0; len];
    let masks = if compression == 3 {
        let p = if dib >= 52 { 14 + 40 } else { 14 + dib };
        [
            u32at(b, p)?,
            u32at(b, p + 4)?,
            u32at(b, p + 8)?,
            if dib >= 56 { u32at(b, p + 12)? } else { 0 },
        ]
    } else {
        [0, 0, 0, 0]
    };
    let palette_start = 14 + dib + if compression == 3 && dib == 40 { 12 } else { 0 };
    let palette_count = if depth <= 8 {
        let n = u32at(b, 46)? as usize;
        if n == 0 { 1usize << depth } else { n }
    } else {
        0
    };
    if palette_count > 256
        || palette_start
            .checked_add(palette_count * 4)
            .is_none_or(|end| end > off || end > b.len())
        || off > b.len()
    {
        return Err(Error::Invalid("BMP palette/offset"));
    }
    let put = |x: usize, y: usize, v: u32, rgba8: &mut [u8]| -> Result<()> {
        if x >= width as usize || y >= height as usize {
            return Err(Error::Invalid("BMP RLE position"));
        }
        let dy = if h > 0 { height as usize - 1 - y } else { y };
        let p = (dy * width as usize + x) * 4;
        let color = if depth <= 8 {
            let i = v as usize;
            if i >= palette_count {
                return Err(Error::Invalid("BMP palette index"));
            }
            let q = palette_start + i * 4;
            [b[q + 2], b[q + 1], b[q], 255]
        } else if compression == 3 {
            [
                component(v, masks[0]),
                component(v, masks[1]),
                component(v, masks[2]),
                component(v, masks[3]),
            ]
        } else {
            [
                ((v >> 16) & 255) as u8,
                ((v >> 8) & 255) as u8,
                (v & 255) as u8,
                if depth == 32 {
                    ((v >> 24) & 255) as u8
                } else {
                    255
                },
            ]
        };
        rgba8[p..p + 4].copy_from_slice(&color);
        Ok(())
    };
    if compression == 1 || compression == 2 {
        let mut p = off;
        let (mut x, mut y) = (0usize, 0usize);
        let mut ended = false;
        while p + 2 <= b.len() && y < height as usize {
            let n = b[p] as usize;
            let c = b[p + 1];
            p += 2;
            if n > 0 {
                for i in 0..n {
                    let v = if depth == 8 {
                        c
                    } else if i % 2 == 0 {
                        c >> 4
                    } else {
                        c & 15
                    };
                    put(x, y, v as u32, &mut rgba8)?;
                    x += 1;
                }
            } else {
                match c {
                    0 => {
                        x = 0;
                        y += 1
                    }
                    1 => {
                        ended = true;
                        break;
                    }
                    2 => {
                        if p + 2 > b.len() {
                            return Err(Error::Invalid("truncated BMP RLE"));
                        }
                        x += b[p] as usize;
                        y += b[p + 1] as usize;
                        p += 2
                    }
                    count => {
                        let count = count as usize;
                        let bytes = if depth == 8 { count } else { count.div_ceil(2) };
                        if p + bytes > b.len() {
                            return Err(Error::Invalid("truncated BMP RLE"));
                        }
                        for i in 0..count {
                            let v = if depth == 8 {
                                b[p + i]
                            } else if i % 2 == 0 {
                                b[p + i / 2] >> 4
                            } else {
                                b[p + i / 2] & 15
                            };
                            put(x, y, v as u32, &mut rgba8)?;
                            x += 1;
                        }
                        p += bytes + (bytes & 1);
                        if p > b.len() {
                            return Err(Error::Invalid("truncated BMP RLE"));
                        }
                    }
                }
            }
        }
        if !ended {
            return Err(Error::Invalid("truncated BMP RLE"));
        }
    } else {
        let stride = (width as usize * depth as usize).div_ceil(32) * 4;
        let end = off
            .checked_add(
                stride
                    .checked_mul(height as usize)
                    .ok_or(Error::Invalid("BMP size"))?,
            )
            .ok_or(Error::Invalid("BMP size"))?;
        if end > b.len() {
            return Err(Error::Invalid("truncated BMP pixels"));
        }
        for y in 0..height as usize {
            let row = &b[off + y * stride..off + (y + 1) * stride];
            for x in 0..width as usize {
                let v = match depth {
                    1 => ((row[x / 8] >> (7 - x % 8)) & 1) as u32,
                    4 => ((row[x / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 15) as u32,
                    8 => row[x] as u32,
                    16 => u16::from_le_bytes(row[x * 2..x * 2 + 2].try_into().unwrap()) as u32,
                    24 => u32::from_le_bytes([row[x * 3], row[x * 3 + 1], row[x * 3 + 2], 0]),
                    32 => u32::from_le_bytes(row[x * 4..x * 4 + 4].try_into().unwrap()),
                    _ => unreachable!(),
                };
                put(x, y, v, &mut rgba8)?;
            }
        }
    }
    Ok(Decoded {
        width,
        height,
        rgba8,
        icc: None,
        metadata: Metadata::default(),
        notices: vec![],
    })
}

pub(crate) fn encode(image: &Decoded, alpha: bool) -> Result<Vec<u8>> {
    let depth = if alpha { 32usize } else { 24 };
    let stride = (image.width as usize * depth).div_ceil(32) * 4;
    let size = 54usize
        .checked_add(
            stride
                .checked_mul(image.height as usize)
                .ok_or(Error::Invalid("BMP size"))?,
        )
        .ok_or(Error::Invalid("BMP size"))?;
    if size > u32::MAX as usize {
        return Err(Error::Invalid("BMP size"));
    }
    let mut b = vec![0u8; size];
    b[..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&(size as u32).to_le_bytes());
    b[10..14].copy_from_slice(&54u32.to_le_bytes());
    b[14..18].copy_from_slice(&40u32.to_le_bytes());
    b[18..22].copy_from_slice(&image.width.to_le_bytes());
    b[22..26].copy_from_slice(&image.height.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&(depth as u16).to_le_bytes());
    b[34..38].copy_from_slice(&((size - 54) as u32).to_le_bytes());
    for y in 0..image.height as usize {
        for x in 0..image.width as usize {
            let src = ((image.height as usize - 1 - y) * image.width as usize + x) * 4;
            let dst = 54 + y * stride + x * (depth / 8);
            b[dst..dst + 3].copy_from_slice(&[
                image.rgba8[src + 2],
                image.rgba8[src + 1],
                image.rgba8[src],
            ]);
            if alpha {
                b[dst + 3] = image.rgba8[src + 3];
            }
        }
    }
    Ok(b)
}

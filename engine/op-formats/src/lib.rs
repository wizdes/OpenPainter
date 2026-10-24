//! Image codecs and the native OpenPainter file format.
pub mod png;

mod bmp;
mod exif_ifd;
mod io;
pub use io::{
    ChromaSubsampling, Decoded, Format, Metadata, Options, PngCompression, TiffCompression, decode,
    encode,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid image: {0}")]
    Invalid(&'static str),
    #[error("codec error: {0}")]
    Codec(String),
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn codec(e: impl std::fmt::Display) -> Error {
    Error::Codec(e.to_string())
}
pub(crate) fn checked_len(w: u32, h: u32) -> Result<usize> {
    if w == 0 || h == 0 || w > 100_000 || h > 100_000 {
        return Err(Error::Invalid("image dimensions exceed limit"));
    }
    let n = (w as u64) * (h as u64) * 4;
    if n > 1_073_741_824 {
        return Err(Error::Invalid("decoded image exceeds 1 GiB"));
    }
    Ok(n as usize)
}

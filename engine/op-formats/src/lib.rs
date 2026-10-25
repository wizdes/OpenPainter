//! Image codecs and the native OpenPainter file format.
pub mod opaint;
pub mod png;

mod bmp;
mod exif_ifd;
mod io;
pub use io::{
    ChromaSubsampling, Decoded, Format, Metadata, Options, PngCompression, TiffCompression, decode,
    encode,
};

/// Where a save really writes, and the permissions it keeps.
pub struct SaveTarget {
    /// `path` with symlinks followed: a save through a symlink replaces the
    /// file it points to and leaves the link (release review item 10).
    pub path: std::path::PathBuf,
    /// The replaced file's permissions, given to the new file before it is
    /// renamed into place. None for a new file.
    pub permissions: Option<std::fs::Permissions>,
}

/// The file a save of `path` replaces (see `SaveTarget`). A link to a file
/// that does not exist yet saves to the place the link names.
pub fn save_target(path: &std::path::Path) -> std::io::Result<SaveTarget> {
    let mut target = path.to_path_buf();
    // At most 40 links, as the system allows.
    for _ in 0..40 {
        match std::fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let link = std::fs::read_link(&target)?;
                target = match target.parent() {
                    Some(parent) if link.is_relative() => parent.join(link),
                    _ => link,
                };
            }
            Ok(meta) => {
                return Ok(SaveTarget {
                    path: target,
                    permissions: Some(meta.permissions()),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SaveTarget {
                    path: target,
                    permissions: None,
                });
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(format!(
        "too many symbolic links at {}",
        path.display()
    )))
}

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

/// M57 fuzz entry for the EXIF IFD reader (the TIFF-structure walk used for
/// TIFF EXIF): parses `bytes`, writes the entries back out and parses that.
/// Returns whether the first parse succeeded. Not part of the file API.
#[doc(hidden)]
pub fn fuzz_exif_ifd(bytes: &[u8]) -> bool {
    match exif_ifd::parse(bytes) {
        Ok(entries) => {
            if let Ok(out) = exif_ifd::serialize(&entries) {
                let _ = exif_ifd::parse(&out);
            }
            true
        }
        Err(_) => false,
    }
}

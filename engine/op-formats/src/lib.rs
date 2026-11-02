//! Image codecs and the native OpenPainter file format.
pub mod opaint;
pub mod png;

/// How long a save waits for another program to let go of the target.
#[cfg(windows)]
const REPLACE_ATTEMPTS: usize = 40;
#[cfg(windows)]
const REPLACE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

/// The path with the `\\?\` prefix, so `MoveFileExW` and `ReplaceFileW` accept
/// it past `MAX_PATH` whatever the calling app's long-path setting is.
#[cfg(windows)]
fn extended_wide(path: &std::path::Path) -> std::io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let absolute = std::path::absolute(path)?;
    let text = absolute.as_os_str().to_string_lossy();
    let prefixed = if text.starts_with(r"\\?\") {
        text.into_owned()
    } else if let Some(share) = text.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{share}")
    } else {
        format!(r"\\?\{text}")
    };
    Ok(std::ffi::OsStr::new(&prefixed)
        .encode_wide()
        .chain(Some(0))
        .collect())
}

/// Flushes a folder's entries. A folder opens only with
/// FILE_FLAG_BACKUP_SEMANTICS, and flushing needs a write handle: a read
/// handle gives error 5 on `sync_all`.
#[cfg(windows)]
pub fn sync_folder(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)?
        .sync_all()
}

/// Puts a fully synced temporary file in the target's place on Windows.
///
/// An existing target goes through `ReplaceFileW`, which keeps its access
/// rules, extra streams and creation time. With a backup name, a failed call
/// always leaves both files under their own names, so a caller that deletes
/// the temporary file after an error never loses the old file. A read-only
/// target is refused at once. A target another program holds is retried for
/// about 2 s, and then the error names the file.
#[cfg(windows)]
pub fn replace_synced_file(
    temp: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    };
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_UNABLE_TO_REMOVE_REPLACED: i32 = 1175;
    const ERROR_UNABLE_TO_MOVE_REPLACEMENT: i32 = 1176;
    const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;
    let named = |error: std::io::Error, why: &str| {
        std::io::Error::new(
            error.kind(),
            format!("cannot replace {}: {error}{why}", target.display()),
        )
    };
    let exists = match std::fs::metadata(target) {
        Ok(meta) if meta.permissions().readonly() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("cannot replace {}: the file is read-only", target.display()),
            ));
        }
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(named(e, "")),
    };
    let from = extended_wide(temp)?;
    let to = extended_wide(target)?;
    let mut backup_name = temp.as_os_str().to_owned();
    backup_name.push(".old");
    let backup_path = std::path::PathBuf::from(backup_name);
    let backup = extended_wide(&backup_path)?;
    let mut replace = exists;
    for attempt in 0..REPLACE_ATTEMPTS {
        let ok = unsafe {
            if replace {
                ReplaceFileW(
                    to.as_ptr(),
                    from.as_ptr(),
                    backup.as_ptr(),
                    0,
                    null_mut(),
                    null(),
                )
            } else {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
        };
        if ok != 0 {
            if replace && let Err(e) = std::fs::remove_file(&backup_path) {
                eprintln!(
                    "saved {}, but could not remove {}: {e}",
                    target.display(),
                    backup_path.display()
                );
            }
            // The save is complete here. Some drives refuse a folder flush,
            // and that must not report a saved file as failed.
            if let Some(parent) = target.parent()
                && let Err(e) = sync_folder(parent)
            {
                eprintln!(
                    "saved {}, but could not flush {}: {e}",
                    target.display(),
                    parent.display()
                );
            }
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        let code = error.raw_os_error();
        if replace && code == Some(ERROR_FILE_NOT_FOUND) && !target.exists() {
            replace = false;
            continue;
        }
        let held = matches!(
            code,
            Some(
                ERROR_ACCESS_DENIED
                    | ERROR_SHARING_VIOLATION
                    | ERROR_UNABLE_TO_REMOVE_REPLACED
                    | ERROR_UNABLE_TO_MOVE_REPLACEMENT
                    | ERROR_UNABLE_TO_MOVE_REPLACEMENT_2
            )
        );
        if !held {
            return Err(named(error, ""));
        }
        if attempt + 1 == REPLACE_ATTEMPTS {
            return Err(named(error, "; another program may have it open"));
        }
        std::thread::sleep(REPLACE_RETRY_DELAY);
    }
    Err(named(
        std::io::Error::other("no attempt finished"),
        "; another program may have it open",
    ))
}

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

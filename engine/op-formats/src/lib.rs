//! Image codecs and the native OpenPainter file format.
pub mod opaint;
pub mod png;

/// How long a save waits for another program to let go of the target.
#[cfg(windows)]
const REPLACE_ATTEMPTS: usize = 40;
#[cfg(windows)]
const REPLACE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

/// The path as wide characters with the `\\?\` prefix, for Windows calls that
/// otherwise stop at `MAX_PATH`. It works on the wide characters directly, so
/// a name with a broken character stays that same name.
#[cfg(windows)]
fn extended_wide(path: &std::path::Path) -> std::io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let absolute = std::path::absolute(path)?;
    let wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let text = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let mut out = if wide.starts_with(&text(r"\\?\")) {
        Vec::new()
    } else if wide.starts_with(&text(r"\\")) {
        text(r"\\?\UNC")
    } else {
        text(r"\\?\")
    };
    if out.ends_with(&text("UNC")) {
        out.extend_from_slice(&wide[1..]);
    } else {
        out.extend_from_slice(&wide);
    }
    out.push(0);
    Ok(out)
}

/// Flushes a folder's entries. A folder opens only with
/// FILE_FLAG_BACKUP_SEMANTICS, and flushing needs a write handle: a read
/// handle gives error 5 on `sync_all`. A drive with no folder flush (error 1,
/// seen on a network share) has nothing to flush.
#[cfg(windows)]
pub fn sync_folder(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const ERROR_INVALID_FUNCTION: i32 = 1;
    let dir = if dir.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        dir
    };
    let result = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)?
        .sync_all();
    match result {
        Err(e) if e.raw_os_error() == Some(ERROR_INVALID_FUNCTION) => Ok(()),
        other => other,
    }
}

/// Moves a fully synced temporary file over the target in one atomic rename,
/// so the target's name always holds the whole old file or the whole new one.
/// `ReplaceFileW` must not be used here: it renames the target away first, and
/// a crash in that gap leaves no file under the name.
///
/// A target another program holds is retried for about 2 s. The temporary file
/// stays in place until the rename succeeds.
#[cfg(windows)]
pub fn rename_synced_file(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    if target.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::IsADirectory,
            format!("cannot replace {}: it is a folder", target.display()),
        ));
    }
    if let Ok(meta) = std::fs::metadata(target)
        && meta.permissions().readonly()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("cannot replace {}: the file is read-only", target.display()),
        ));
    }
    let mut attempt = 0;
    loop {
        // On current Windows this uses POSIX rename rules, so a reader that
        // shares delete access does not block it.
        match std::fs::rename(temp, target) {
            Ok(()) => break,
            Err(e)
                if attempt + 1 < REPLACE_ATTEMPTS
                    && matches!(
                        e.raw_os_error(),
                        Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)
                    ) =>
            {
                attempt += 1;
                std::thread::sleep(REPLACE_RETRY_DELAY);
            }
            Err(e) => {
                return Err(std::io::Error::new(
                    e.kind(),
                    format!(
                        "cannot replace {}: {e}; another program may have it open, or access may be denied",
                        target.display()
                    ),
                ));
            }
        }
    }
    let parent = target.parent().unwrap_or(std::path::Path::new("."));
    sync_folder(parent)
}

/// Copies what a save must keep from the old file onto the temporary file:
/// access rules, extra streams, creation time and the hidden and system
/// attributes. Without this, a private file becomes readable by more accounts
/// after a save. Access rules that cannot be kept stop the save. A lost stream
/// or date is printed and the save goes on.
#[cfg(windows)]
fn copy_file_details(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::fs::{FileTimesExt, MetadataExt};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, FindClose, FindFirstStreamW, FindNextStreamW,
        FindStreamInfoStandard, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
        WIN32_FIND_STREAM_DATA,
    };
    let old = extended_wide(from)?;
    let new = extended_wide(to)?;
    let failed = |what: &str, code: u32| {
        std::io::Error::new(
            std::io::Error::from_raw_os_error(code as i32).kind(),
            format!(
                "cannot keep the access rules of {}: {what}: {}",
                from.display(),
                std::io::Error::from_raw_os_error(code as i32)
            ),
        )
    };
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let read = unsafe {
        GetNamedSecurityInfoW(
            old.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if read != ERROR_SUCCESS {
        return Err(failed("read", read));
    }
    let set = unsafe {
        let mut control = 0u16;
        let mut revision = 0u32;
        let protected = GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) != 0
            && control & SE_DACL_PROTECTED != 0;
        // A drive with no access rules has no list to copy.
        if dacl.is_null() {
            ERROR_SUCCESS
        } else {
            SetNamedSecurityInfoW(
                new.as_ptr().cast_mut(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION
                    | if protected {
                        PROTECTED_DACL_SECURITY_INFORMATION
                    } else {
                        UNPROTECTED_DACL_SECURITY_INFORMATION
                    },
                null_mut(),
                null_mut(),
                dacl,
                null(),
            )
        }
    };
    unsafe { LocalFree(descriptor) };
    if set != ERROR_SUCCESS {
        return Err(failed("write", set));
    }

    let mut data: WIN32_FIND_STREAM_DATA = unsafe { std::mem::zeroed() };
    let find = unsafe {
        FindFirstStreamW(
            old.as_ptr(),
            FindStreamInfoStandard,
            (&raw mut data).cast(),
            0,
        )
    };
    if find as isize != -1 && !find.is_null() {
        loop {
            let len = data.cStreamName.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&data.cStreamName[..len]);
            if name != "::$DATA"
                && let Some(short) = name.strip_suffix(":$DATA")
            {
                let with = |p: &std::path::Path| {
                    let mut s = p.as_os_str().to_owned();
                    s.push(short);
                    std::path::PathBuf::from(s)
                };
                // CopyFile treats a stream name as a whole file, so copy the bytes.
                if let Err(e) = std::fs::read(with(from)).and_then(|b| std::fs::write(with(to), b))
                {
                    eprintln!("could not keep stream {short} of {}: {e}", from.display());
                }
            }
            if unsafe { FindNextStreamW(find, (&raw mut data).cast()) } == 0 {
                break;
            }
        }
        unsafe { FindClose(find) };
    }

    let meta = std::fs::metadata(from)?;
    let attributes = unsafe { GetFileAttributesW(old.as_ptr()) };
    if attributes != INVALID_FILE_ATTRIBUTES {
        let keep = attributes & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
        let now = meta.file_attributes();
        if keep != 0 && unsafe { SetFileAttributesW(new.as_ptr(), now | keep) } == 0 {
            eprintln!(
                "could not keep the attributes of {}: {}",
                from.display(),
                std::io::Error::last_os_error()
            );
        }
    }
    match meta.created() {
        Ok(created) => {
            let times = std::fs::FileTimes::new().set_created(created);
            let result = std::fs::OpenOptions::new()
                .write(true)
                .open(to)
                .and_then(|f| f.set_times(times));
            if let Err(e) = result {
                eprintln!(
                    "could not keep the creation time of {}: {e}",
                    from.display()
                );
            }
        }
        Err(e) => eprintln!(
            "could not read the creation time of {}: {e}",
            from.display()
        ),
    }
    Ok(())
}

/// Puts a fully synced temporary file in place of the user's file. It keeps
/// the old file's details, and then swaps the two in one atomic rename.
#[cfg(windows)]
pub fn replace_synced_file(
    temp: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    if std::fs::metadata(target).is_ok_and(|m| m.is_file()) {
        copy_file_details(target, temp)?;
    }
    rename_synced_file(temp, target)
}

#[cfg(all(test, windows))]
mod windows_replace_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn the_target_name_never_goes_missing_during_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("work.opaint");
        std::fs::write(&target, b"start").unwrap();
        let done = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let watcher = scope.spawn(|| {
                let mut checks = 0u64;
                while !done.load(Ordering::SeqCst) {
                    assert!(std::fs::metadata(&target).is_ok(), "the name went missing");
                    checks += 1;
                }
                checks
            });
            for n in 0..300 {
                let temp = dir.path().join(format!("{n}.tmp"));
                std::fs::write(&temp, n.to_string()).unwrap();
                replace_synced_file(&temp, &target).unwrap();
            }
            done.store(true, Ordering::SeqCst);
            assert!(watcher.join().unwrap() > 100);
        });
        assert_eq!(std::fs::read(&target).unwrap(), b"299");
        let names = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(names, 1, "nothing is left beside the target");
    }

    #[test]
    fn a_name_with_a_broken_character_stays_that_name() {
        use std::os::windows::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let name = std::ffi::OsString::from_wide(&[0x70, 0xD800, 0x2E, 0x6F]);
        let target = dir.path().join(&name);
        let temp = dir.path().join("t.tmp");
        std::fs::write(&temp, b"new").unwrap();
        replace_synced_file(&temp, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn a_folder_target_fails_at_once_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("folder");
        std::fs::create_dir(&target).unwrap();
        let temp = dir.path().join("t.tmp");
        std::fs::write(&temp, b"new").unwrap();
        let started = std::time::Instant::now();
        let error = replace_synced_file(&temp, &target).unwrap_err();
        assert!(error.to_string().contains("folder"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert!(temp.exists());
    }
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

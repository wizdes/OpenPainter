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

/// The targets a save must refuse at once, with a clear reason.
#[cfg(windows)]
fn refuse_target(target: &std::path::Path) -> std::io::Result<()> {
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
    Ok(())
}

/// Moves a fully synced temporary file over the target in one atomic rename,
/// so the target's name always holds the whole old file or the whole new one.
/// `ReplaceFileW` must not be used here: it renames the target away first, and
/// a crash in that gap leaves no file under the name.
///
/// A target another program holds is retried for about 2 s. The temporary file
/// stays in place until the rename succeeds. This does not flush the folder:
/// an error from here always means the target did not change.
#[cfg(windows)]
pub fn rename_synced_file(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    refuse_target(target)?;
    with_retries(|| std::fs::rename(temp, target)).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "cannot replace {}: {e}; another program may have it open, or access may be denied",
                target.display()
            ),
        )
    })
}

/// Runs `work` until it stops failing with "access denied" or "sharing
/// violation", which is what a program holding the file gives, for about 2 s.
#[cfg(windows)]
fn with_retries<T>(mut work: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    let mut attempt = 0;
    loop {
        match work() {
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
            other => return other,
        }
    }
}

/// The error codes for a drive that has no access rules at all.
#[cfg(windows)]
fn no_access_rules_here(error: &std::io::Error) -> bool {
    const ERROR_INVALID_FUNCTION: i32 = 1;
    const ERROR_NOT_SUPPORTED: i32 = 50;
    matches!(
        error.raw_os_error(),
        Some(ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED)
    )
}

/// The file's access rules, exactly as they are. None: this drive has none.
#[cfg(windows)]
fn read_access_rules(path: &std::path::Path) -> std::io::Result<Option<Vec<u8>>> {
    use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, GetFileSecurityW};
    let wide = extended_wide(path)?;
    let mut needed = 0u32;
    unsafe {
        GetFileSecurityW(
            wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if needed == 0 {
        let error = std::io::Error::last_os_error();
        return if no_access_rules_here(&error) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let mut buffer = vec![0u8; needed as usize];
    let ok = unsafe {
        GetFileSecurityW(
            wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Some(buffer))
}

/// Sets exactly these rules. Unlike `SetNamedSecurityInfoW`, this does not
/// rebuild inherited rules from the current folder, which would widen access
/// on a file moved in from a private folder.
#[cfg(windows)]
fn write_access_rules(path: &std::path::Path, rules: *const u8) -> std::io::Result<()> {
    use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, SetFileSecurityW};
    let wide = extended_wide(path)?;
    let ok = unsafe {
        SetFileSecurityW(
            wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            rules.cast_mut().cast(),
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Makes a new temporary file private to the account saving, before any bytes
/// go into it, when it will replace an existing file. The old file may be
/// private, and the folder's rules may let others read. The owner keeps full
/// control, so the file can always be finished or deleted.
#[cfg(windows)]
pub fn make_temp_private(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    const SDDL_REVISION_1: u32 = 1;
    if !std::fs::metadata(target).is_ok_and(|m| m.is_file()) {
        return Ok(());
    }
    // Protected list: the owner and SYSTEM only.
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)\0".encode_utf16().collect();
    let mut descriptor = std::ptr::null_mut();
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = write_access_rules(temp, descriptor as *const u8);
    unsafe { LocalFree(descriptor) };
    match result {
        Err(e) if no_access_rules_here(&e) => Ok(()),
        other => other,
    }
}

/// Copies every extra stream from the old file to the temporary file. A stream
/// that cannot be kept stops the save, and the old file stays as it was.
#[cfg(windows)]
fn copy_streams(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FindClose, FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard,
        WIN32_FIND_STREAM_DATA,
    };
    const ERROR_INVALID_PARAMETER: i32 = 87;
    const ERROR_HANDLE_EOF: i32 = 38;
    let old = extended_wide(from)?;
    let suffix: Vec<u16> = ":$DATA".encode_utf16().collect();
    let mut data: WIN32_FIND_STREAM_DATA = unsafe { std::mem::zeroed() };
    let find = unsafe {
        FindFirstStreamW(
            old.as_ptr(),
            FindStreamInfoStandard,
            (&raw mut data).cast(),
            0,
        )
    };
    if find as isize == -1 {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            // No streams, or a drive that does not have them.
            Some(ERROR_HANDLE_EOF | ERROR_INVALID_PARAMETER) => Ok(()),
            _ if no_access_rules_here(&error) => Ok(()),
            _ => Err(error),
        };
    }
    let result = (|| loop {
        let len = data
            .cStreamName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(data.cStreamName.len());
        let name = &data.cStreamName[..len];
        // "::$DATA" is the file's main content.
        if len > suffix.len() + 1 && name.ends_with(&suffix) {
            let short = std::ffi::OsString::from_wide(&name[..len - suffix.len()]);
            let with = |p: &std::path::Path| {
                let mut s = p.as_os_str().to_owned();
                s.push(&short);
                std::path::PathBuf::from(s)
            };
            with_retries(|| {
                let bytes = std::fs::read(with(from))?;
                let mut file = std::fs::File::create(with(to))?;
                std::io::Write::write_all(&mut file, &bytes)?;
                file.sync_all()
            })
            .map_err(|e| {
                std::io::Error::new(
                    e.kind(),
                    format!(
                        "cannot keep stream {} of {}: {e}",
                        short.display(),
                        from.display()
                    ),
                )
            })?;
        }
        if unsafe { FindNextStreamW(find, (&raw mut data).cast()) } == 0 {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_HANDLE_EOF) {
                Ok(())
            } else {
                Err(error)
            };
        }
    })();
    unsafe { FindClose(find) };
    result
}

/// Puts a fully synced temporary file in place of the user's file.
///
/// Order matters. Streams, creation time and attributes go onto the temporary
/// file first, while it is private and fully ours. The old rules are read
/// before the rename, and applied after it. A crash between those two leaves
/// the file private to the saver: too narrow, and never wider. Not kept:
/// owner, mandatory label, compression and encryption.
#[cfg(windows)]
pub fn replace_synced_file(
    temp: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::fs::{FileTimesExt, MetadataExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, SetFileAttributesW,
    };
    const ERROR_ACCESS_DENIED: i32 = 5;
    refuse_target(target)?;
    let mut rules = None;
    if let Ok(meta) = std::fs::metadata(target)
        && meta.is_file()
    {
        if let Err(e) = std::fs::OpenOptions::new().write(true).open(target)
            && e.raw_os_error() == Some(ERROR_ACCESS_DENIED)
        {
            return Err(std::io::Error::new(
                e.kind(),
                format!("cannot replace {}: you may not change it", target.display()),
            ));
        }
        rules = read_access_rules(target).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("cannot keep the access rules of {}: {e}", target.display()),
            )
        })?;
        copy_streams(target, temp)?;
        let keep = meta.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
        if keep != 0 {
            let now = std::fs::metadata(temp)?.file_attributes();
            let wide = extended_wide(temp)?;
            if unsafe { SetFileAttributesW(wide.as_ptr(), now | keep) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        if let Ok(created) = meta.created() {
            let times = std::fs::FileTimes::new().set_created(created);
            with_retries(|| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(temp)?
                    .set_times(times)
            })?;
        }
    }
    rename_synced_file(temp, target)?;
    // The save is complete. Neither failure below may report it as failed,
    // so each is printed with what it means.
    if let Some(rules) = &rules
        && let Err(e) = write_access_rules(target, rules.as_ptr())
    {
        eprintln!(
            "saved {}, but could not restore its access rules, so it stays private to you: {e}",
            target.display()
        );
    }
    let parent = target.parent().unwrap_or(std::path::Path::new("."));
    if let Err(e) = sync_folder(parent) {
        eprintln!(
            "saved {}, but could not flush {}: {e}",
            target.display(),
            parent.display()
        );
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_replace_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Stops the watcher even when a save panics, so the test fails and does
    /// not hang.
    struct StopOnDrop<'a>(&'a AtomicBool);
    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

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
            {
                let _stop = StopOnDrop(&done);
                for n in 0..300 {
                    let temp = dir.path().join(format!("{n}.tmp"));
                    std::fs::write(&temp, n.to_string()).unwrap();
                    replace_synced_file(&temp, &target).unwrap();
                }
            }
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
        for content in [&b"one"[..], b"two"] {
            let temp = dir.path().join("t.tmp");
            std::fs::write(&temp, content).unwrap();
            replace_synced_file(&temp, &target).unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), content);
        }
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

    fn acl_of(path: &std::path::Path) -> String {
        let out = std::process::Command::new("icacls")
            .arg(path)
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8_lossy(&out.stdout)
            .replace(&*path.to_string_lossy(), "")
            .trim()
            .to_owned()
    }

    fn icacls(path: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("icacls")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}");
    }

    #[test]
    fn rules_inherited_from_a_private_folder_do_not_widen_after_a_move() {
        let root = tempfile::tempdir().unwrap();
        let user = std::env::var("USERNAME").unwrap();
        let private = root.path().join("private");
        let open = root.path().join("open");
        std::fs::create_dir(&private).unwrap();
        std::fs::create_dir(&open).unwrap();
        icacls(
            &private,
            &["/inheritance:r", "/grant:r", &format!("{user}:(OI)(CI)F")],
        );
        icacls(&open, &["/grant", "Users:(OI)(CI)R"]);
        let inside = private.join("a.opaint");
        std::fs::write(&inside, b"old").unwrap();
        let target = open.join("a.opaint");
        std::fs::rename(&inside, &target).unwrap();
        let before = acl_of(&target);
        assert!(!before.contains("Users"), "{before}");
        let temp = open.join("t.tmp");
        std::fs::write(&temp, b"new").unwrap();
        make_temp_private(&temp, &target).unwrap();
        replace_synced_file(&temp, &target).unwrap();
        assert_eq!(acl_of(&target), before);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn the_temporary_file_is_private_before_any_bytes_go_in() {
        let root = tempfile::tempdir().unwrap();
        icacls(root.path(), &["/grant", "Users:(OI)(CI)R"]);
        let target = root.path().join("a.opaint");
        std::fs::write(&target, b"old").unwrap();
        let temp = root.path().join("t.tmp");
        std::fs::write(&temp, b"").unwrap();
        assert!(acl_of(&temp).contains("Users"));
        make_temp_private(&temp, &target).unwrap();
        let acl = acl_of(&temp);
        assert!(!acl.contains("Users"), "{acl}");
        std::fs::write(&temp, b"still writable").unwrap();
        std::fs::remove_file(&temp).unwrap();
    }

    #[test]
    fn a_target_without_a_delete_right_fails_and_leaves_the_temp_removable() {
        let root = tempfile::tempdir().unwrap();
        let user = std::env::var("USERNAME").unwrap();
        let target = root.path().join("a.opaint");
        std::fs::write(&target, b"old").unwrap();
        icacls(
            &target,
            &["/inheritance:r", "/grant:r", &format!("{user}:(R,W)")],
        );
        let temp = root.path().join("t.tmp");
        std::fs::write(&temp, b"new").unwrap();
        make_temp_private(&temp, &target).unwrap();
        // Rights on the folder may still allow the swap. Either way nothing is lost.
        match replace_synced_file(&temp, &target) {
            Ok(()) => assert_eq!(std::fs::read(&target).unwrap(), b"new"),
            Err(_) => {
                assert_eq!(std::fs::read(&target).unwrap(), b"old");
                std::fs::remove_file(&temp).unwrap();
            }
        }
    }

    #[test]
    fn a_target_that_denies_writing_is_refused_with_details_kept() {
        let root = tempfile::tempdir().unwrap();
        let user = std::env::var("USERNAME").unwrap();
        let target = root.path().join("a.opaint");
        std::fs::write(&target, b"old").unwrap();
        icacls(&target, &["/deny", &format!("{user}:(WD,AD)")]);
        let temp = root.path().join("t.tmp");
        std::fs::write(&temp, b"new").unwrap();
        let error = replace_synced_file(&temp, &target).unwrap_err();
        assert!(error.to_string().contains("may not change"), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        icacls(&target, &["/remove:d", &user]);
    }

    #[test]
    fn a_rename_error_means_the_target_did_not_change() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("a.opaint");
        std::fs::write(&target, b"old").unwrap();
        let missing = root.path().join("no-temp.tmp");
        assert!(rename_synced_file(&missing, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn a_save_through_a_network_path_works_when_the_share_exists() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().to_string_lossy().into_owned();
        let Some(rest) = local.strip_prefix("C:\\") else {
            eprintln!("NOT RUN: temp folder is not on C:");
            return;
        };
        let share = std::path::PathBuf::from(format!(r"\\localhost\C$\{rest}"));
        if !share.exists() {
            eprintln!("NOT RUN: \\\\localhost\\C$ is not available");
            return;
        }
        let target = share.join("a.opaint");
        for content in [&b"one"[..], b"two"] {
            let temp = share.join("t.tmp");
            std::fs::write(&temp, content).unwrap();
            replace_synced_file(&temp, &target).unwrap();
            assert_eq!(std::fs::read(dir.path().join("a.opaint")).unwrap(), content);
        }
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

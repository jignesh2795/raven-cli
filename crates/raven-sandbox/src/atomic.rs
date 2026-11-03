//! Atomic file replacement.
//!
//! A file edit must never leave a truncated or partially written file behind.
//! The replacement is written to a temporary file in the same directory and
//! then swapped in with `ReplaceFileW` (existing target) or `MoveFileExW` with
//! `MOVEFILE_WRITE_THROUGH` (new target). Both calls are transactional at the
//! filesystem level: they either complete or leave the original untouched.
//!
//! This module is the only place in the workspace allowed to use `unsafe`. The
//! calls below are security-critical and live next to the future containment
//! code, so every other crate can stay under `#![deny(unsafe_code)]`.

#![cfg_attr(windows, allow(unsafe_code))]

use std::io;
use std::path::Path;

/// Replace `target` with `replacement`, or create `target` when it does not
/// exist. `replacement` must be fully written and synced and must be on the
/// same volume as `target`.
///
/// If the swap cannot be performed atomically this returns an error rather than
/// falling back to delete-then-rename, which could lose the original content.
pub fn persist(replacement: &Path, target: &Path) -> io::Result<()> {
    imp::persist(replacement, target)
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
    };

    pub fn persist(replacement: &Path, target: &Path) -> io::Result<()> {
        let source = wide(replacement)?;
        let destination = wide(target)?;

        if target.exists() {
            // SAFETY: both paths are NUL-terminated UTF-16 buffers that outlive
            // the call. `std::ptr::null()` is valid for the optional backup,
            // exclude, and reserved arguments.
            let replaced = unsafe {
                ReplaceFileW(
                    destination.as_ptr(),
                    source.as_ptr(),
                    std::ptr::null(),
                    REPLACEFILE_WRITE_THROUGH,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            };
            if replaced != 0 {
                return Ok(());
            }

            let error = io::Error::last_os_error();
            let code = error.raw_os_error().map(|code| code as u32);
            if code != Some(ERROR_FILE_NOT_FOUND) && code != Some(ERROR_PATH_NOT_FOUND) {
                return Err(error);
            }
            // The target disappeared between `exists()` and `ReplaceFileW`.
            // Fall through to the create-only path, which cannot clobber a file
            // another process may have created in the meantime.
        }

        // SAFETY: the buffers are NUL-terminated and outlive the call.
        // MOVEFILE_WRITE_THROUGH flushes the rename before returning, and the
        // missing REPLACE_EXISTING flag makes an unexpected target an error.
        let moved = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut buffer: Vec<u16> = path.as_os_str().encode_wide().collect();
        if buffer.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains a NUL character",
            ));
        }
        buffer.push(0);
        Ok(buffer)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;
    use std::path::Path;

    pub fn persist(replacement: &Path, target: &Path) -> io::Result<()> {
        // On Unix, rename(2) is atomic and replaces the destination.
        std::fs::rename(replacement, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn replaces_an_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let replacement = temp.path().join("replacement.txt");
        fs::write(&target, "old content").unwrap();
        fs::write(&replacement, "new content").unwrap();

        persist(&replacement, &target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
        assert!(!replacement.exists());
    }

    #[test]
    fn creates_a_missing_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("created.txt");
        let replacement = temp.path().join("replacement.txt");
        fs::write(&replacement, "fresh").unwrap();

        persist(&replacement, &target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "fresh");
        assert!(!replacement.exists());
    }
}

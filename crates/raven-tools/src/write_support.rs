//! Shared helpers for the write tools: UTF-8/BOM/EOL handling, hashing, and the
//! atomic temp-file write.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use raven_core::{Error, Result};

pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

pub struct TextFile {
    pub text: String,
    pub bom: bool,
}

pub fn read_text_file(path: &Path) -> Result<TextFile> {
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Tool(format!(
                "'{}' does not exist; use create_file for new files",
                path.display()
            ))
        } else {
            Error::Io(error)
        }
    })?;
    decode(&bytes, path)
}

pub fn decode(bytes: &[u8], path: &Path) -> Result<TextFile> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Error::Tool(format!(
            "'{}' is larger than the {} MiB edit limit",
            path.display(),
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(Error::Tool(format!(
            "'{}' is UTF-16; convert it to UTF-8 before editing",
            path.display()
        )));
    }

    let bom = bytes.starts_with(UTF8_BOM);
    let body = if bom { &bytes[UTF8_BOM.len()..] } else { bytes };
    if body.iter().take(8192).any(|byte| *byte == 0) {
        return Err(Error::Tool(format!(
            "'{}' looks like a binary file",
            path.display()
        )));
    }

    let text = std::str::from_utf8(body).map_err(|error| {
        Error::Tool(format!("'{}' is not valid UTF-8 ({error})", path.display()))
    })?;
    Ok(TextFile {
        text: text.to_string(),
        bom,
    })
}

pub fn encode(text: &str, bom: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + if bom { UTF8_BOM.len() } else { 0 });
    if bom {
        bytes.extend_from_slice(UTF8_BOM);
    }
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    raven_core::util::sha256_hex(bytes)
}

/// Write `bytes` to a temporary file in the target directory, sync it, verify
/// it, and swap it into place atomically. The original is untouched if any step
/// before the swap fails.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Tool("path has no parent directory".into()))?;
    let temp = parent.join(format!(".raven-tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut guard = TempFile::new(temp.clone());

    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    let written = fs::read(&temp)?;
    if written.as_slice() != bytes {
        return Err(Error::Tool(
            "temporary file verification failed; the original was not changed".into(),
        ));
    }

    raven_sandbox::atomic::persist(&temp, path).map_err(|error| {
        Error::Tool(format!(
            "could not replace '{}' atomically: {error}; the original was not changed",
            path.display()
        ))
    })?;
    guard.disarm();
    Ok(())
}

/// Remove the temporary file unless the swap succeeded.
struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Return the dominant line ending. Windows files are usually CRLF, but
/// repositories on Windows frequently contain LF files, so this is measured
/// rather than assumed.
pub fn detect_eol(text: &str) -> &'static str {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count();
    if crlf > lf - crlf {
        "\r\n"
    } else {
        "\n"
    }
}

pub fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

pub fn to_eol(text: &str, eol: &str) -> String {
    if eol == "\r\n" {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

pub fn line_number(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

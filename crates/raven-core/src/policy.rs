use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tool::ToolKind;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyMode {
    /// Only read-only tools are allowed. No write tool is registered.
    ReadOnly,
    /// Exact file edits confined to the workspace are allowed. Every edit is
    /// shown to the human and requires approval unless `--yes` is passed.
    WorkspaceWrite,
}

impl PolicyMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
        }
    }

    pub fn allows_writes(self) -> bool {
        matches!(self, Self::WorkspaceWrite)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    mode: PolicyMode,
}

impl Default for Policy {
    fn default() -> Self {
        Self::read_only()
    }
}

impl Policy {
    pub fn read_only() -> Self {
        Self {
            mode: PolicyMode::ReadOnly,
        }
    }

    pub fn workspace_write() -> Self {
        Self {
            mode: PolicyMode::WorkspaceWrite,
        }
    }

    pub fn mode(&self) -> PolicyMode {
        self.mode
    }

    /// Check whether a tool of this kind may run under the current policy.
    /// Denials are returned to the model as tool errors by the agent loop.
    pub fn check_tool(&self, kind: ToolKind) -> Result<()> {
        match (self.mode, kind) {
            (_, ToolKind::ReadOnly) => Ok(()),
            (PolicyMode::WorkspaceWrite, ToolKind::Write) => Ok(()),
            (PolicyMode::ReadOnly, ToolKind::Write) => Err(Error::Policy(
                "read-only mode: file edits are disabled (remove --read-only or set policy = \"workspace-write\")".into(),
            )),
            (_, ToolKind::Execute) => Err(Error::Policy(
                "command execution is not implemented yet (M2)".into(),
            )),
            (_, ToolKind::Network) => Err(Error::Policy(
                "network access for tools is not implemented yet".into(),
            )),
        }
    }
}

/// Confines reads to a canonical workspace root and denies known secret-bearing
/// paths. The OS-level boundary arrives in M3 (AppContainer/MXC); this policy is
/// still required for a useful default and is tested independently.
#[derive(Debug, Clone)]
pub struct PathPolicy {
    root: PathBuf,
}

impl PathPolicy {
    pub fn new(root: &Path) -> Result<Self> {
        let canonical = dunce::canonicalize(root).map_err(|error| {
            Error::Config(format!(
                "workspace '{}' is not accessible: {error}",
                root.display()
            ))
        })?;
        Ok(Self { root: canonical })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve_existing(&self, raw: &str) -> Result<PathBuf> {
        if raw.trim().is_empty() {
            return Err(Error::Tool("path is empty".into()));
        }

        if looks_like_device_path(raw) {
            return Err(Error::Policy(format!("device path is not allowed: {raw}")));
        }

        let raw_path = Path::new(raw);
        let candidate = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            self.root.join(raw_path)
        };

        let canonical = dunce::canonicalize(&candidate)
            .map_err(|error| Error::Tool(format!("cannot access '{}': {error}", raw)))?;

        if !self.is_within(&canonical) {
            return Err(Error::Policy(format!("path escapes the workspace: {raw}")));
        }

        if is_protected_path(&canonical) {
            return Err(Error::Policy(format!("path is protected by policy: {raw}")));
        }

        Ok(canonical)
    }

    /// Resolve a path that is about to be written. The leaf may not exist yet,
    /// so the parent directory is canonicalized (resolving symlinks and
    /// junctions) and the name itself is validated against Windows rules.
    /// Callers must re-check the target immediately before replacing it.
    pub fn resolve_for_write(&self, raw: &str) -> Result<PathBuf> {
        if raw.trim().is_empty() {
            return Err(Error::Tool("path is empty".into()));
        }

        if looks_like_device_path(raw) {
            return Err(Error::Policy(format!("device path is not allowed: {raw}")));
        }

        let raw_path = Path::new(raw);
        let candidate = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            self.root.join(raw_path)
        };

        // Existing entries are fully canonicalized so that symlinks, junctions,
        // and alternate names are resolved before the policy checks.
        if std::fs::symlink_metadata(&candidate).is_ok() {
            let canonical = dunce::canonicalize(&candidate)
                .map_err(|error| Error::Tool(format!("cannot resolve '{}': {error}", raw)))?;
            if !self.is_within(&canonical) {
                return Err(Error::Policy(format!("path escapes the workspace: {raw}")));
            }
            if is_protected_path(&canonical) {
                return Err(Error::Policy(format!("path is protected by policy: {raw}")));
            }
            if canonical.is_dir() {
                return Err(Error::Tool(format!("'{}' is a directory", raw)));
            }
            validate_leaf(&canonical)?;
            return Ok(canonical);
        }

        let parent = candidate
            .parent()
            .ok_or_else(|| Error::Tool(format!("path has no parent directory: {raw}")))?;
        let name = candidate
            .file_name()
            .ok_or_else(|| Error::Tool(format!("path has no file name: {raw}")))?;
        validate_leaf_name(name)?;

        let parent = dunce::canonicalize(parent).map_err(|error| {
            Error::Tool(format!(
                "cannot access the parent directory of '{}': {error}",
                raw
            ))
        })?;
        if !self.is_within(&parent) {
            return Err(Error::Policy(format!("path escapes the workspace: {raw}")));
        }

        let path = parent.join(name);
        if is_protected_path(&path) {
            return Err(Error::Policy(format!("path is protected by policy: {raw}")));
        }
        Ok(path)
    }

    pub fn is_within(&self, path: &Path) -> bool {
        let root = self.root.to_string_lossy();
        let candidate = path.to_string_lossy();
        if cfg!(windows) {
            let root = root.to_lowercase();
            let candidate = candidate.to_lowercase();
            candidate == root
                || candidate.starts_with(&format!("{}{}", root, std::path::MAIN_SEPARATOR))
        } else {
            candidate == root
                || candidate.starts_with(&format!("{}{}", root, std::path::MAIN_SEPARATOR))
        }
    }

    pub fn display_relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

fn looks_like_device_path(raw: &str) -> bool {
    let upper = raw.to_ascii_uppercase();
    upper.starts_with(r"\\?\GLOBALROOT")
        || raw.starts_with(r"\\.\")
        || upper.starts_with(r"\\?\PIPE\")
}

const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5",
    "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9",
];

fn validate_leaf(path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| Error::Tool("path has no file name".into()))?;
    validate_leaf_name(name)
}

fn validate_leaf_name(name: &std::ffi::OsStr) -> Result<()> {
    let name = name.to_string_lossy();

    if name.is_empty() {
        return Err(Error::Tool("file name is empty".into()));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(Error::Tool(format!(
            "file name has a trailing dot or space, which Windows would strip: {name:?}"
        )));
    }
    if name.contains(':') {
        return Err(Error::Policy(format!(
            "colons are not allowed in file names (alternate data streams): {name:?}"
        )));
    }
    if name.chars().any(|character| {
        character.is_control() || matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
    }) {
        return Err(Error::Tool(format!(
            "file name contains a character Windows does not allow: {name:?}"
        )));
    }

    let stem = name.split('.').next().unwrap_or(&name).trim_end();
    let upper = stem.to_ascii_uppercase();
    if RESERVED_NAMES.contains(&upper.as_str()) {
        return Err(Error::Tool(format!(
            "file name is reserved by Windows: {name:?}"
        )));
    }
    Ok(())
}

/// Reduce a file name to the form Windows would actually open: alternate data
/// stream suffix removed, trailing dots/spaces stripped, and lowercased. This
/// keeps the denylist from being bypassed by `.env::$DATA` or `key.pem.`.
fn normalize_name(name: &str) -> String {
    let base = name.split(':').next().unwrap_or(name);
    base.trim_end_matches(['.', ' ']).to_ascii_lowercase()
}

pub fn is_protected_path(path: &Path) -> bool {
    for component in path.components() {
        if normalize_name(&component.as_os_str().to_string_lossy()) == ".git" {
            return true;
        }
    }

    let name = path
        .file_name()
        .map(|name| normalize_name(&name.to_string_lossy()))
        .unwrap_or_default();

    if name == ".env" || name.starts_with(".env.") {
        return true;
    }
    if matches!(
        name.as_str(),
        "id_rsa"
            | "id_dsa"
            | "id_ecdsa"
            | "id_ed25519"
            | ".npmrc"
            | ".pypirc"
            | "credentials"
            | "secrets"
            | "secrets.json"
            | "secrets.toml"
    ) {
        return true;
    }
    [".pem", ".key", ".pfx", ".p12", ".kdbx"]
        .iter()
        .any(|extension| name.ends_with(extension))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn denies_paths_outside_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("inside.txt"), "ok").unwrap();
        fs::write(temp.path().join("outside.txt"), "no").unwrap();

        let policy = PathPolicy::new(&workspace).unwrap();
        policy.resolve_existing("inside.txt").unwrap();
        assert!(policy.resolve_existing("../outside.txt").is_err());
    }

    #[test]
    fn denies_protected_paths() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join(".env"), "SECRET=1").unwrap();
        fs::write(temp.path().join("key.pem"), "secret").unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();
        fs::write(temp.path().join(".git").join("config"), "").unwrap();

        let policy = PathPolicy::new(temp.path()).unwrap();
        assert!(policy.resolve_existing(".env").is_err());
        assert!(policy.resolve_existing("key.pem").is_err());
        assert!(policy.resolve_existing(".git/config").is_err());
    }

    #[test]
    fn protected_names_cannot_be_bypassed_with_ads_or_trailing_dots() {
        assert!(is_protected_path(Path::new(r"C:\work\.env::$DATA")));
        assert!(is_protected_path(Path::new(r"C:\work\key.pem.")));
        assert!(is_protected_path(Path::new(r"C:\work\.git ")));
        assert!(is_protected_path(Path::new(r"C:\work\secrets.json:stream")));
        assert!(!is_protected_path(Path::new(r"C:\work\.environment")));
        assert!(!is_protected_path(Path::new(r"C:\work\notes.txt")));
    }

    #[test]
    fn write_policy_confines_new_files_and_rejects_windows_aliases() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(workspace.join("src")).unwrap();
        fs::create_dir(workspace.join("docs")).unwrap();

        let policy = PathPolicy::new(&workspace).unwrap();
        assert!(policy.resolve_for_write("src/new.rs").is_ok());
        assert!(policy.resolve_for_write("new.rs").is_ok());
        assert!(policy.resolve_for_write("../escape.rs").is_err());
        assert!(policy.resolve_for_write(".env").is_err());
        assert!(policy.resolve_for_write("docs/key.pem").is_err());
        assert!(policy.resolve_for_write("CON").is_err());
        assert!(policy.resolve_for_write("aux.txt").is_err());
        assert!(policy.resolve_for_write("trailing. ").is_err());
        assert!(policy.resolve_for_write("stream.txt:secret").is_err());
        assert!(policy.resolve_for_write("missing/new.rs").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn write_policy_rejects_a_symlinked_file_outside_the_workspace() {
        use std::os::windows::fs::symlink_file;

        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let outside = temp.path().join("outside.txt");
        fs::write(&outside, "outside").unwrap();

        let link = workspace.join("link.txt");
        if symlink_file(&outside, &link).is_err() {
            return; // symlink privilege is unavailable in this environment
        }

        let policy = PathPolicy::new(&workspace).unwrap();
        assert!(policy.resolve_for_write("link.txt").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn write_policy_rejects_writes_through_a_directory_junction() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&outside).unwrap();

        let link = workspace.join("link");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .status();
        let created = status.map(|status| status.success()).unwrap_or(false) && link.exists();
        if !created {
            return; // junction creation is unavailable in this environment
        }

        let policy = PathPolicy::new(&workspace).unwrap();
        assert!(policy.resolve_for_write("link/new.txt").is_err());
        assert!(policy.resolve_existing("link").is_err());
    }
}

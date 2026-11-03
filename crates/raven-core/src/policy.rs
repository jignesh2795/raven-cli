use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tool::ToolKind;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    /// Only read-only tools are allowed. This is the M0 default.
    ReadOnly,
    /// File writes confined to the workspace are allowed. Not implemented in M0.
    WorkspaceWrite,
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
                "this build is read-only; file edits are not available".into(),
            )),
            (_, ToolKind::Execute) => Err(Error::Policy(
                "command execution is not available in M0".into(),
            )),
            (_, ToolKind::Network) => Err(Error::Policy(
                "network access for tools is not available in M0".into(),
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

pub fn is_protected_path(path: &Path) -> bool {
    for component in path.components() {
        if component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(".git")
        {
            return true;
        }
    }

    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
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
}

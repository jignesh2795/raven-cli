//! Workspace trust gate.
//!
//! A project-local `raven.toml` may provide custom-provider endpoints and
//! non-credential defaults. It cannot select the default provider, choose the
//! credential environment variable, change pricing, or disable redaction. A
//! cloned repository's config is ignored until the workspace is explicitly
//! trusted.
//!
//! Trust is stored per canonical workspace path and pinned to the SHA-256 of
//! the exact `raven.toml` bytes that were reviewed. Editing the project file
//! invalidates the record, so a repository that later changes its own config
//! has to be reviewed again.
//!
//! Trust is a user decision, not a filesystem capability. It is honored by this
//! process only: a malicious process running as the same user can edit
//! `trust.json`. That containment requires an OS boundary and belongs to M3.

use std::path::{Path, PathBuf};

use raven_core::policy::PolicyMode;
use raven_core::redact::{sanitize_text, Redactor};
use raven_core::{Error, Result};
use serde::{Deserialize, Serialize};

const TRUST_FILE: &str = "trust.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustRecord {
    /// Canonical workspace path the record applies to.
    pub workspace: String,
    /// SHA-256 of the project-local `raven.toml` that was reviewed.
    pub config_sha256: String,
    /// RFC 3339 timestamp of the decision.
    pub trusted_at: String,
}

#[derive(Debug, Default)]
pub struct TrustStore {
    records: Vec<TrustRecord>,
}

impl TrustStore {
    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join(TRUST_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let records: Vec<TrustRecord> = serde_json::from_str(&text).map_err(|error| {
            Error::Config(format!(
                "invalid trust store '{}': {error}",
                terminal_safe(&path.display().to_string())
            ))
        })?;
        Ok(Self { records })
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let path = data_dir.join(TRUST_FILE);
        let bytes = serde_json::to_vec_pretty(&self.records)?;
        raven_tools::atomic_write(&path, &bytes).map_err(|error| {
            Error::Config(format!(
                "could not write the trust store '{}': {error}",
                terminal_safe(&path.display().to_string())
            ))
        })?;
        Ok(())
    }

    pub fn is_trusted(&self, workspace: &Path, config_sha256: &str) -> bool {
        let workspace = normalize(workspace);
        self.records
            .iter()
            .any(|record| record.workspace == workspace && record.config_sha256 == config_sha256)
    }

    /// Record trust for this workspace and config hash, replacing any existing
    /// record for the same path.
    pub fn grant(&mut self, workspace: &Path, config_sha256: &str) {
        self.revoke(workspace);
        self.records.push(TrustRecord {
            workspace: normalize(workspace),
            config_sha256: config_sha256.to_string(),
            trusted_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// Remove any record for this workspace. The project file stops being read.
    pub fn revoke(&mut self, workspace: &Path) {
        let workspace = normalize(workspace);
        self.records.retain(|record| record.workspace != workspace);
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn records(&self) -> &[TrustRecord] {
        &self.records
    }
}

/// Comparable workspace key: canonical when the path exists, otherwise the
/// literal path so that a missing workspace still produces a stable record.
fn normalize(workspace: &Path) -> String {
    let canonical = dunce::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    canonical.to_string_lossy().to_lowercase()
}

pub fn config_sha256(bytes: &[u8]) -> String {
    raven_core::util::sha256_hex(bytes)
}

/// Canonical location of a project-local config.
pub fn project_config_path(workspace: &Path) -> PathBuf {
    workspace.join("raven.toml")
}

/// Current trust state for a workspace, without loading any config.
pub fn project_config_state(workspace: &Path) -> ProjectState {
    let data_dir = match crate::config::data_dir() {
        Ok(dir) => dir,
        Err(_) => return ProjectState::Untrusted,
    };
    let path = project_config_path(workspace);
    if !path.exists() {
        return ProjectState::NotFound;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return ProjectState::Untrusted;
    };
    let hash = config_sha256(&bytes);
    match TrustStore::load(&data_dir) {
        Ok(store) if store.is_trusted(workspace, &hash) => ProjectState::Trusted,
        _ => ProjectState::Untrusted,
    }
}

/// Outcome of the trust gate for a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectState {
    /// No project-local `raven.toml`.
    NotFound,
    /// A project file exists but this workspace is not trusted for its current
    /// contents. It is ignored.
    Untrusted,
    /// The workspace is trusted for this exact file; its settings were applied.
    Trusted,
}

impl ProjectState {
    /// Shown at the start of a run so a trusted project config is never silent.
    pub fn notice(&self) -> Option<String> {
        match self {
            Self::NotFound | Self::Trusted => None,
            Self::Untrusted => Some(
                "ignored project-local raven.toml: this workspace is not trusted. \
                 Run `raven trust` to review and trust it."
                    .into(),
            ),
        }
    }

    /// Recorded in the session log.
    pub fn label(self) -> &'static str {
        match self {
            Self::NotFound => "none",
            Self::Untrusted => "ignored",
            Self::Trusted => "applied",
        }
    }
}

/// `raven trust [--grant | --revoke] [--workspace PATH]`
pub fn command(workspace: PathBuf, grant: bool, revoke: bool) -> Result<()> {
    let workspace = dunce::canonicalize(&workspace).map_err(|error| {
        Error::Config(format!(
            "workspace '{}' is not accessible: {error}",
            terminal_safe(&workspace.display().to_string())
        ))
    })?;
    let data_dir = crate::config::data_dir()?;

    if revoke {
        let mut store = TrustStore::load(&data_dir)?;
        store.revoke(&workspace);
        store.save(&data_dir)?;
        println!(
            "trust revoked for {}\n  project-local raven.toml will be ignored again",
            terminal_safe(&workspace.display().to_string())
        );
        return Ok(());
    }

    let path = project_config_path(&workspace);
    if !path.exists() {
        println!(
            "no project-local raven.toml at {}",
            terminal_safe(&path.display().to_string())
        );
        list_records(&data_dir)?;
        return Ok(());
    }

    let bytes = std::fs::read(&path)?;
    let project = crate::config::Config::load_raw(&bytes, &path)?;
    let hash = config_sha256(&bytes);

    let mut store = TrustStore::load(&data_dir)?;
    if store.is_trusted(&workspace, &hash) {
        println!(
            "{} is trusted for its current raven.toml",
            terminal_safe(&workspace.display().to_string())
        );
        show_project_config(&project);
        if grant {
            return Ok(());
        }
        return Ok(());
    }

    if !grant {
        // Neither grant nor revoke: report the current state and stop. Never
        // prompt for a trust decision the user did not ask for.
        println!(
            "{} is NOT trusted",
            terminal_safe(&workspace.display().to_string())
        );
        println!("  project-local raven.toml is being ignored. It would change:");
        show_project_config(&project);
        println!("\n  run `raven trust --grant` to review and trust it");
        list_records(&data_dir)?;
        return Ok(());
    }

    // Granting requires an explicit, informed confirmation of what the file
    // would control. Fail closed without a terminal.
    println!(
        "This will trust the project-local config in:\n  {}",
        terminal_safe(&path.display().to_string())
    );
    show_project_config(&project);
    if !confirm("trust this workspace? [y]es / [n]o: ")? {
        println!("not trusted; nothing was written");
        return Ok(());
    }

    store.grant(&workspace, &hash);
    store.save(&data_dir)?;
    println!(
        "trusted {}\n  pinned to sha256 {}",
        terminal_safe(&workspace.display().to_string()),
        &hash[..16]
    );
    Ok(())
}

fn show_project_config(project: &crate::config::Config) {
    println!(
        "  - default_provider: {} (ignored; user/CLI-owned)",
        terminal_safe(project.default_provider.as_deref().unwrap_or("(unset)"))
    );

    println!("  - providers (custom entries / unset non-credential fields only):");
    for (name, provider) in &project.providers {
        println!(
            "      {}: base_url={} model={} api_key_env={} (ignored; user-owned)",
            terminal_safe(name),
            terminal_safe_endpoint(provider.base_url.as_deref().unwrap_or("(unset)")),
            terminal_safe(provider.model.as_deref().unwrap_or("(unset)")),
            terminal_safe(provider.api_key_env.as_deref().unwrap_or("(unset)"))
        );
    }

    println!(
        "  - policy: {}",
        fmt_opt(project.policy.map(PolicyMode::label))
    );
    println!("  - max_turns: {}", fmt_opt(project.max_turns));
    println!("  - max_cost_usd: {}", fmt_opt(project.max_cost_usd));
    println!("  - output_cap: {}", fmt_opt(project.output_cap));
    println!("  - temperature: {}", fmt_opt(project.temperature));
    println!("  - max_tokens: {}", fmt_opt(project.max_tokens));
    println!(
        "  - pricing: {} project entries ignored (user-owned)",
        project.pricing.len()
    );
    println!(
        "  - redact: {} (project value ignored; user-owned setting only)",
        fmt_opt(project.redact)
    );
}

/// Sanitize and redact values from project files and persistent state before
/// they are displayed in the user's terminal.
fn terminal_safe(value: &str) -> String {
    Redactor::default().redact(&sanitize_text(value))
}

/// Display a provider endpoint without URL userinfo, query parameters, or
/// fragments. These parts commonly carry credentials or access tokens.
fn terminal_safe_endpoint(value: &str) -> String {
    let sanitized = sanitize_text(value);
    let Ok(mut url) = url::Url::parse(&sanitized) else {
        return terminal_safe(&sanitized);
    };
    let omitted_sensitive_parts = !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);

    let endpoint = terminal_safe(url.as_str());
    if omitted_sensitive_parts {
        format!("{endpoint} [REDACTED URL USERINFO/QUERY]")
    } else {
        endpoint
    }
}

fn fmt_opt<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "(unset)".into())
}

fn list_records(data_dir: &Path) -> Result<()> {
    let store = TrustStore::load(data_dir)?;
    if store.is_empty() {
        println!("\nno trusted workspaces");
        return Ok(());
    }
    println!("\ntrusted workspaces:");
    for record in store.records() {
        println!(
            "  {} · {} · since {}",
            terminal_safe(&record.workspace),
            terminal_safe(
                record
                    .config_sha256
                    .get(..16)
                    .unwrap_or(&record.config_sha256)
            ),
            terminal_safe(&record.trusted_at)
        );
    }
    Ok(())
}

fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{IsTerminal, Write};
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(Error::Config(
            "stdin is not a terminal, so trust was not granted".into(),
        ));
    }
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let mut line = String::new();
    if stdin.read_line(&mut line)? == 0 {
        return Ok(false);
    }
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_is_pinned_to_the_config_hash() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("work");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut store = TrustStore::default();
        assert!(!store.is_trusted(&workspace, "aaaa"));

        store.grant(&workspace, "aaaa");
        assert!(
            store.is_trusted(&workspace, "aaaa"),
            "granted hash must match"
        );
        assert!(
            !store.is_trusted(&workspace, "bbbb"),
            "a changed project file must invalidate trust"
        );

        // Granting again replaces the record rather than duplicating it.
        store.grant(&workspace, "bbbb");
        assert!(store.is_trusted(&workspace, "bbbb"));
        assert!(!store.is_trusted(&workspace, "aaaa"));
        assert_eq!(store.records().len(), 1);

        store.revoke(&workspace);
        assert!(!store.is_trusted(&workspace, "bbbb"));
        assert!(store.is_empty());
    }

    #[test]
    fn trust_round_trips_through_disk() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("work");
        std::fs::create_dir_all(&workspace).unwrap();

        let mut store = TrustStore::default();
        store.grant(&workspace, "cccc");
        store.save(temp.path()).unwrap();

        let reloaded = TrustStore::load(temp.path()).unwrap();
        assert!(reloaded.is_trusted(&workspace, "cccc"));

        // A missing store is not an error: an untrusted workspace is the default.
        let empty_dir = tempfile::tempdir().unwrap();
        assert!(TrustStore::load(empty_dir.path()).unwrap().is_empty());
    }

    #[test]
    fn config_hash_changes_with_content() {
        assert_ne!(config_sha256(b"model = 'a'"), config_sha256(b"model = 'b'"));
        assert_eq!(config_sha256(b"same").len(), 64);
    }
}

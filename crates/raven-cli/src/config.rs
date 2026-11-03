use std::collections::BTreeMap;
use std::path::PathBuf;

use raven_core::cost::Pricing;
use raven_core::policy::PolicyMode;
use raven_core::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub default_provider: Option<String>,
    /// `read-only` or `workspace-write`. Defaults to workspace-write; every
    /// write still requires approval unless `--yes` is passed.
    #[serde(default)]
    pub policy: Option<PolicyMode>,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub pricing: BTreeMap<String, Pricing>,
    /// `None` means the user did not set it, so a trusted project layer may.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<usize>,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_cap: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// `None` means the user did not set it, so a trusted project layer may.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redact: Option<bool>,
}

impl Config {
    /// The value the loop uses, after considering user and project layers.
    pub fn effective_max_turns(&self) -> usize {
        self.max_turns.unwrap_or_else(default_max_turns)
    }

    /// The cap the loop uses.
    pub fn effective_output_cap(&self) -> usize {
        self.output_cap.unwrap_or_else(default_output_cap)
    }

    /// Redaction on unless somebody turned it off.
    pub fn effective_redact(&self) -> bool {
        self.redact.unwrap_or(true)
    }

    /// Policy the run should use: explicit setting, else workspace-write.
    pub fn effective_policy(&self) -> PolicyMode {
        self.policy.unwrap_or(PolicyMode::WorkspaceWrite)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_provider: Some("openai".into()),
            policy: Some(PolicyMode::WorkspaceWrite),
            providers: BTreeMap::new(),
            pricing: BTreeMap::new(),
            max_turns: None,
            max_cost_usd: None,
            output_cap: None,
            temperature: None,
            max_tokens: None,
            redact: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Set to false for gateways that reject `stream_options`.
    #[serde(default)]
    pub stream_usage: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub stream_usage: bool,
}

struct ProviderPreset {
    base_url: &'static str,
    model: &'static str,
    api_key_env: Option<&'static str>,
}

fn preset(name: &str) -> Option<ProviderPreset> {
    match name {
        "openai" => Some(ProviderPreset {
            base_url: "https://api.openai.com/v1",
            model: "gpt-4.1",
            api_key_env: Some("OPENAI_API_KEY"),
        }),
        "nim" => Some(ProviderPreset {
            base_url: "https://integrate.api.nvidia.com/v1",
            model: "meta/llama-3.3-70b-instruct",
            api_key_env: Some("NVIDIA_API_KEY"),
        }),
        "ollama" => Some(ProviderPreset {
            base_url: "http://localhost:11434/v1",
            model: "qwen2.5-coder:7b",
            api_key_env: None,
        }),
        _ => None,
    }
}

pub fn preset_names() -> Vec<String> {
    vec!["openai".into(), "nim".into(), "ollama".into()]
}

impl Config {
    pub fn load(explicit: Option<PathBuf>) -> Result<Self> {
        let path = config_path(explicit)?;
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::from_file(&path)
    }

    fn from_file(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text)
            .map_err(|error| Error::Config(format!("invalid config '{}': {error}", path.display())))
    }

    /// Parse config bytes. Used for the project-local layer, which is read from
    /// an explicit workspace rather than a user-controlled path.
    pub fn load_raw(bytes: &[u8], path: &std::path::Path) -> Result<Self> {
        let text = String::from_utf8_lossy(bytes);
        toml::from_str(&text)
            .map_err(|error| Error::Config(format!("invalid config '{}': {error}", path.display())))
    }

    /// Fields a project-local config is allowed to contribute. Credential
    /// selection (`default_provider`, `api_key_env`) and the keyring service
    /// are deliberately absent: a repository must not be able to redirect where
    /// your key comes from or where your code is sent.
    ///
    /// Every field is only filled when the user has not set it, so a trusted
    /// project can supply a model or endpoint but never override the user's.
    fn apply_project_layer(&mut self, project: &Config) {
        if self.max_turns.is_none() {
            self.max_turns = project.max_turns;
        }
        if self.max_cost_usd.is_none() {
            self.max_cost_usd = project.max_cost_usd;
        }
        if self.output_cap.is_none() {
            self.output_cap = project.output_cap;
        }
        if self.temperature.is_none() {
            self.temperature = project.temperature;
        }
        if self.max_tokens.is_none() {
            self.max_tokens = project.max_tokens;
        }
        if self.redact.is_none() {
            self.redact = project.redact;
        }
        if self.policy.is_none() {
            self.policy = project.policy;
        }

        for (name, provider) in &project.providers {
            self.providers.insert(name.clone(), provider.clone());
        }
        for (model, pricing) in &project.pricing {
            self.pricing.insert(model.clone(), *pricing);
        }
    }

    /// Load the user config and, when the workspace is trusted for its current
    /// project-local file, layer the project config underneath it.
    ///
    /// Precedence is CLI flags > user config > project config > presets, so a
    /// trusted project can fill in a model or endpoint the user has not set,
    /// but can never override one the user has set.
    pub fn load_resolved(
        explicit: Option<PathBuf>,
        workspace: &std::path::Path,
    ) -> Result<(Self, crate::trust::ProjectState)> {
        Self::load_resolved_in(explicit, workspace, &crate::config::data_dir()?)
    }

    /// Testable core of [`Self::load_resolved`] with an explicit data
    /// directory for the trust store.
    pub fn load_resolved_in(
        explicit: Option<PathBuf>,
        workspace: &std::path::Path,
        data_dir: &std::path::Path,
    ) -> Result<(Self, crate::trust::ProjectState)> {
        let mut config = Self::load(explicit)?;

        let project_path = crate::trust::project_config_path(workspace);
        if !project_path.exists() {
            return Ok((config, crate::trust::ProjectState::NotFound));
        }
        let bytes = std::fs::read(&project_path)?;
        let project = Self::load_raw(&bytes, &project_path)?;
        let hash = crate::trust::config_sha256(&bytes);

        let store = crate::trust::TrustStore::load(data_dir)?;
        if !store.is_trusted(workspace, &hash) {
            return Ok((config, crate::trust::ProjectState::Untrusted));
        }

        config.apply_project_layer(&project);
        Ok((config, crate::trust::ProjectState::Trusted))
    }

    pub fn resolve_provider(
        &self,
        name: &str,
        base_url_override: Option<String>,
        model_override: Option<String>,
        api_key_env_override: Option<String>,
    ) -> Result<ResolvedProvider> {
        let configured = self.providers.get(name);
        let preset = preset(name);

        let base_url = base_url_override
            .or_else(|| configured.and_then(|provider| provider.base_url.clone()))
            .or_else(|| preset.as_ref().map(|preset| preset.base_url.to_string()))
            .ok_or_else(|| {
                Error::Config(format!(
                    "unknown provider '{name}'; configure [providers.{name}] with base_url and model"
                ))
            })?;

        let model = model_override
            .or_else(|| configured.and_then(|provider| provider.model.clone()))
            .or_else(|| preset.as_ref().map(|preset| preset.model.to_string()))
            .ok_or_else(|| {
                Error::Config(format!(
                    "no model for provider '{name}'; pass --model or set providers.{name}.model"
                ))
            })?;

        let api_key_env = api_key_env_override
            .or_else(|| configured.and_then(|provider| provider.api_key_env.clone()))
            .or_else(|| {
                preset
                    .as_ref()
                    .and_then(|preset| preset.api_key_env.map(str::to_string))
            });

        let stream_usage = configured
            .and_then(|provider| provider.stream_usage)
            .unwrap_or(true);

        Ok(ResolvedProvider {
            name: name.to_string(),
            base_url,
            model,
            api_key_env,
            stream_usage,
        })
    }

    pub fn known_provider_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.providers.keys().cloned().collect();
        for name in preset_names() {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.sort();
        names
    }
}

pub fn config_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Ok(path) = std::env::var("RAVEN_CONFIG") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let base = dirs::config_dir()
        .ok_or_else(|| Error::Config("could not locate the user configuration directory".into()))?;
    Ok(base.join("raven").join("raven.toml"))
}

pub fn data_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("RAVEN_DATA_DIR") {
        if !path.trim().is_empty() {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path)?;
            return Ok(path);
        }
    }
    let base = dirs::data_local_dir().ok_or_else(|| {
        Error::Config("could not locate the local application data directory".into())
    })?;
    let path = base.join("raven");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn init(explicit: Option<PathBuf>, force: bool) -> Result<PathBuf> {
    let path = config_path(explicit)?;
    if path.exists() && !force {
        return Err(Error::Config(format!(
            "{} already exists; pass --force to replace it",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, sample_toml())?;
    Ok(path)
}

pub fn sample_toml() -> String {
    r#"# Raven CLI configuration.
# API keys are not stored here: use `raven auth set <provider>`.
# Project-local raven.toml files are ignored until you run `raven trust` for
# that workspace. Trust is pinned to the exact file contents, so editing the
# project file requires re-trusting it.

default_provider = "openai"
max_turns = 12
output_cap = 65536
redact = true
# "workspace-write" (default) allows exact search/replace edits inside the
# workspace. Every edit is shown as a diff and needs approval unless --yes.
# "read-only" refuses all writes.
policy = "workspace-write"
# temperature = 0.2
# max_tokens = 4096
# max_cost_usd = 1.50

[providers.openai]
base_url = "https://api.openai.com/v1"
model = "gpt-4.1"
api_key_env = "OPENAI_API_KEY"

[providers.nim]
base_url = "https://integrate.api.nvidia.com/v1"
model = "meta/llama-3.3-70b-instruct"
api_key_env = "NVIDIA_API_KEY"

[providers.ollama]
base_url = "http://localhost:11434/v1"
model = "qwen2.5-coder:7b"
stream_usage = false

# Prices are USD per million tokens and are examples, not live vendor quotes.
# Add an entry for every model you pass to --max-cost.
[pricing."gpt-4.1"]
input_per_mtok = 2.00
output_per_mtok = 8.00
cached_input_per_mtok = 0.50

[pricing."qwen2.5-coder:7b"]
input_per_mtok = 0.00
output_per_mtok = 0.00
cached_input_per_mtok = 0.00
"#
    .to_string()
}

fn default_max_turns() -> usize {
    12
}

fn default_output_cap() -> usize {
    64 * 1024
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn user_config(dir: &Path) -> PathBuf {
        let path = dir.join("user.toml");
        // Only default_provider is set: everything else stays None so the
        // project layer is free to fill it.
        fs::write(&path, "default_provider = \"openai\"\n").unwrap();
        path
    }

    /// A user config that pins a value, to prove the user always wins.
    fn user_config_with(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("pinned.toml");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn an_untrusted_workspace_is_never_merged() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap();
        let user = user_config(temp.path());
        fs::write(
            workspace.join("raven.toml"),
            "model = \"evil/v1\"\nmax_turns = 3\n",
        )
        .unwrap();

        let (config, state) =
            Config::load_resolved_in(Some(user), &workspace, temp.path()).unwrap();

        assert_eq!(state, crate::trust::ProjectState::Untrusted);
        assert_eq!(
            config.effective_max_turns(),
            12,
            "project value must not leak in"
        );
        assert!(
            config.providers.is_empty(),
            "project providers must not load"
        );
    }

    #[test]
    fn trust_lets_the_project_config_fill_unset_fields() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap();
        let user = user_config(temp.path());
        let project = "max_turns = 3\npolicy = \"read-only\"\n\n[providers.local]\nbase_url = \"http://localhost:1234/v1\"\nmodel = \"local\"\n";
        fs::write(workspace.join("raven.toml"), project).unwrap();

        let mut store = crate::trust::TrustStore::default();
        store.grant(&workspace, &crate::trust::config_sha256(project.as_bytes()));
        store.save(temp.path()).unwrap();

        let (config, state) =
            Config::load_resolved_in(Some(user), &workspace, temp.path()).unwrap();

        assert_eq!(state, crate::trust::ProjectState::Trusted);
        assert_eq!(
            config.effective_max_turns(),
            3,
            "project fills a field the user left unset"
        );
        assert_eq!(config.effective_policy(), PolicyMode::ReadOnly);
        assert!(config.providers.contains_key("local"));
    }

    #[test]
    fn the_user_config_wins_over_a_trusted_project_file() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap();
        // The user sets max_turns and redact explicitly; the project must not
        // override them.
        let user = user_config_with(
            temp.path(),
            "default_provider = \"openai\"\nmax_turns = 20\nredact = true\n",
        );
        let project = "max_turns = 3\nredact = false\npolicy = \"read-only\"\n";
        fs::write(workspace.join("raven.toml"), project).unwrap();

        let mut store = crate::trust::TrustStore::default();
        store.grant(&workspace, &crate::trust::config_sha256(project.as_bytes()));
        store.save(temp.path()).unwrap();

        let (config, state) =
            Config::load_resolved_in(Some(user), &workspace, temp.path()).unwrap();

        assert_eq!(state, crate::trust::ProjectState::Trusted);
        assert_eq!(config.effective_max_turns(), 20, "the user config must win");
        assert!(
            config.effective_redact(),
            "the user's redact = true must survive"
        );
        assert_eq!(config.effective_policy(), PolicyMode::ReadOnly);
    }

    #[test]
    fn editing_the_project_file_invalidates_trust() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap();
        let user = user_config(temp.path());
        let trusted_text = "max_turns = 3\n";
        fs::write(workspace.join("raven.toml"), trusted_text).unwrap();

        let mut store = crate::trust::TrustStore::default();
        store.grant(
            &workspace,
            &crate::trust::config_sha256(trusted_text.as_bytes()),
        );
        store.save(temp.path()).unwrap();

        let (_, state) =
            Config::load_resolved_in(Some(user.clone()), &workspace, temp.path()).unwrap();
        assert_eq!(state, crate::trust::ProjectState::Trusted);

        // The repository rewrites its own config: trust must no longer apply.
        fs::write(
            workspace.join("raven.toml"),
            "max_turns = 3\nredact = false\n",
        )
        .unwrap();
        let (config, state) =
            Config::load_resolved_in(Some(user), &workspace, temp.path()).unwrap();
        assert_eq!(state, crate::trust::ProjectState::Untrusted);
        assert!(
            config.effective_redact(),
            "the changed file must not be applied"
        );
    }

    #[test]
    fn a_missing_project_file_is_not_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap();
        let (config, state) =
            Config::load_resolved_in(Some(user_config(temp.path())), &workspace, temp.path())
                .unwrap();
        assert_eq!(state, crate::trust::ProjectState::NotFound);
        assert_eq!(config.effective_max_turns(), 12);
    }
}

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
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default = "default_output_cap")]
    pub output_cap: usize,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default = "default_true")]
    pub redact: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_provider: Some("openai".into()),
            policy: Some(PolicyMode::WorkspaceWrite),
            providers: BTreeMap::new(),
            pricing: BTreeMap::new(),
            max_turns: default_max_turns(),
            max_cost_usd: None,
            output_cap: default_output_cap(),
            temperature: None,
            max_tokens: None,
            redact: true,
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
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text)
            .map_err(|error| Error::Config(format!("invalid config '{}': {error}", path.display())))
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
# Project-local raven.toml files are intentionally ignored until the
# workspace trust gate exists; a cloned repository must not control endpoints.

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

fn default_true() -> bool {
    true
}

use std::io::Read;

use keyring::Entry;
use raven_core::{Error, Result};

use crate::config::{Config, ResolvedProvider};

pub const SERVICE: &str = "raven-cli";

pub fn resolve_key(
    provider: &ResolvedProvider,
    api_key_env_override: Option<&str>,
) -> Result<Option<String>> {
    if let Ok(key) = std::env::var("RAVEN_API_KEY") {
        if !key.trim().is_empty() {
            return Ok(Some(key));
        }
    }

    let env_name = api_key_env_override
        .map(str::to_string)
        .or_else(|| provider.api_key_env.clone());
    let env_hint = env_name.clone().unwrap_or_else(|| "RAVEN_API_KEY".into());
    if let Some(env_name) = env_name {
        if let Ok(key) = std::env::var(&env_name) {
            if !key.trim().is_empty() {
                return Ok(Some(key));
            }
        }
    }

    match entry(&provider.name)?.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(Some(key)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => {
            if is_local(&provider.base_url) {
                Ok(None)
            } else {
                Err(Error::Config(format!(
                    "no API key for '{}'; run `raven auth set {}` or set {}",
                    provider.name, provider.name, env_hint
                )))
            }
        }
        Err(error) => Err(Error::Config(format!(
            "Windows Credential Manager error for '{}': {error}",
            provider.name
        ))),
    }
}

pub fn set(provider: &str, read_stdin: bool) -> Result<()> {
    let key = if read_stdin {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer.trim().to_string()
    } else {
        rpassword::prompt_password(format!("API key for {provider}: "))
            .map_err(|error| Error::Config(format!("could not read API key: {error}")))?
            .trim()
            .to_string()
    };

    if key.is_empty() {
        return Err(Error::Config("API key is empty".into()));
    }

    entry(provider)?
        .set_password(&key)
        .map_err(|error| Error::Config(format!("could not store API key: {error}")))?;
    println!("stored key for {provider} in Windows Credential Manager");
    Ok(())
}

pub fn status(config: &Config) -> Result<()> {
    let providers = config.known_provider_names();
    println!("{:<12} {:<12} SOURCE", "PROVIDER", "CREDENTIAL");
    for provider in providers {
        let env_name = config
            .providers
            .get(&provider)
            .and_then(|provider| provider.api_key_env.clone());
        let env_set = env_key_present(env_name.as_deref());
        let keyring_set = match entry(&provider)?.get_password() {
            Ok(value) => !value.trim().is_empty(),
            Err(keyring::Error::NoEntry) => false,
            Err(error) => {
                println!("{provider:<12} error        {error}");
                continue;
            }
        };

        let (credential, source) = if env_set {
            ("set", "environment")
        } else if keyring_set {
            ("set", "Credential Manager")
        } else if is_local(
            &config
                .resolve_provider(&provider, None, None, None)
                .map(|provider| provider.base_url)
                .unwrap_or_default(),
        ) {
            ("not needed", "local endpoint")
        } else {
            ("missing", "run `raven auth set`")
        };
        println!("{provider:<12} {credential:<12} {source}");
    }
    Ok(())
}

pub fn delete(provider: &str) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) => println!("deleted key for {provider}"),
        Err(keyring::Error::NoEntry) => println!("no stored key for {provider}"),
        Err(error) => return Err(Error::Config(format!("could not delete API key: {error}"))),
    }
    Ok(())
}

pub fn has_key(provider: &str) -> Result<bool> {
    match entry(provider)?.get_password() {
        Ok(value) => Ok(!value.trim().is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(Error::Config(format!(
            "Windows Credential Manager error: {error}"
        ))),
    }
}

/// Whether an environment variable (or the global `RAVEN_API_KEY`) supplies a
/// key for this provider. Values are never returned or logged.
pub fn env_key_present(api_key_env: Option<&str>) -> bool {
    env_var_set("RAVEN_API_KEY") || api_key_env.map(env_var_set).unwrap_or(false)
}

fn env_var_set(name: &str) -> bool {
    std::env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn entry(provider: &str) -> Result<Entry> {
    Entry::new(SERVICE, provider)
        .map_err(|error| Error::Config(format!("could not open credential store: {error}")))
}

/// Whether a provider URL explicitly targets a loopback endpoint.
///
/// Parse the URL and inspect its host. Substring checks on the full URL would
/// incorrectly treat remote hosts or paths containing "localhost" as local.
pub fn is_local(base_url: &str) -> bool {
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }

    match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_local;

    #[test]
    fn recognizes_only_exact_loopback_hosts() {
        for url in [
            "http://localhost:11434/v1",
            "https://LOCALHOST/v1",
            "http://127.0.0.1:8080/v1",
            "http://127.255.255.254:8080/v1",
            "http://[::1]:8080/v1",
        ] {
            assert!(is_local(url), "expected local endpoint: {url}");
        }

        for url in [
            "ftp://localhost:21/resource",
            "file:///tmp/localhost",
            "http://[::]:8080/v1",
            "https://localhost.attacker.example/v1",
            "https://127.0.0.1.attacker.example/v1",
            "https://attacker.example/localhost",
            "https://attacker.example/?next=127.0.0.1",
            "http://0.0.0.0:8080/v1",
            "not a URL",
            "/relative/path/localhost",
        ] {
            assert!(!is_local(url), "remote or malformed endpoint accepted: {url}");
        }
    }
}

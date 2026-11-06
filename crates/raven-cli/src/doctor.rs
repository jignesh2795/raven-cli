use std::path::PathBuf;
use std::process::Command;

use raven_core::redact::{sanitize_text, Redactor};
use raven_core::Result;

use crate::auth;
use crate::config::{self, Config};

pub fn run(config_path: Option<PathBuf>) -> Result<()> {
    let config = Config::load(config_path.clone())?;
    let path = config::config_path(config_path)?;
    let data = config::data_dir()?;
    let workspace = std::env::current_dir()?;

    println!("raven {}", env!("CARGO_PKG_VERSION"));
    println!(
        "os:        {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!(
        "config:    {} [{}]",
        terminal_safe(&path.display().to_string()),
        if path.exists() { "found" } else { "not found" }
    );
    println!("data:      {}", terminal_safe(&data.display().to_string()));
    println!("workspace: {}", terminal_safe(&workspace.display().to_string()));
    println!(
        "policy:    {} (edits require approval unless --yes)",
        config.effective_policy().label()
    );
    let trust: String = match crate::trust::project_config_state(&workspace) {
        crate::trust::ProjectState::NotFound => "no project-local raven.toml".into(),
        crate::trust::ProjectState::Untrusted => {
            "project-local raven.toml present but IGNORED (run `raven trust`)".into()
        }
        crate::trust::ProjectState::Trusted => {
            "project-local raven.toml trusted and applied".into()
        }
    };
    println!("trust:     {trust}");
    println!("git:       {}", git_version());
    println!(
        "agents:    {}",
        if workspace.join("AGENTS.md").exists() {
            "AGENTS.md found"
        } else {
            "no AGENTS.md"
        }
    );

    println!("\nproviders:");
    for name in config.known_provider_names() {
        let resolved = config.resolve_provider(&name, None, None, None);
        let endpoint = resolved
            .as_ref()
            .map(|provider| terminal_safe(&provider.base_url))
            .unwrap_or_else(|error| terminal_safe(&format!("<{error}>")));
        let env_name = resolved
            .as_ref()
            .ok()
            .and_then(|provider| provider.api_key_env.as_deref());
        let credential = if auth::env_key_present(env_name) {
            "environment"
        } else if auth::has_key(&name).unwrap_or(false) {
            "stored"
        } else if resolved
            .as_ref()
            .map(|provider| auth::is_local(&provider.base_url))
            .unwrap_or(false)
        {
            "not needed (local)"
        } else {
            "missing"
        };
        println!("  {:<10} {credential:<20} {endpoint}", terminal_safe(&name));
    }

    println!("\nsandbox:");
    let probe = raven_sandbox::probe();
    println!("  mxc:          {}", describe(&probe.mxc));
    println!("  appcontainer: {}", describe(&probe.appcontainer));
    println!("  best-effort:  {}", describe(&probe.best_effort));
    println!("  vm:           {}", describe(&probe.vm));

    println!("\nnotes:");
    println!("  - File edits are exact search/replace, atomic, and hash-verified.");
    println!("  - No command execution backend is active (planned for M2).");
    println!("  - Run from a normal (non-elevated) terminal. Elevation is refused at M3.");
    println!("  - Add [pricing.\"<model>\"] to raven.toml before using --max-cost.");

    Ok(())
}

fn describe(result: &raven_sandbox::ProbeResult) -> String {
    match result {
        raven_sandbox::ProbeResult::Available => "available".into(),
        raven_sandbox::ProbeResult::Unavailable(reason) => format!("unavailable: {}", terminal_safe(reason)),
        raven_sandbox::ProbeResult::Unknown(reason) => format!("unknown: {}", terminal_safe(reason)),
        raven_sandbox::ProbeResult::Planned(reason) => format!("planned: {}", terminal_safe(reason)),
    }
}


fn terminal_safe(input: &str) -> String {
    Redactor::default().redact(&sanitize_text(input))
}

fn git_version() -> String {
    match Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => {
            terminal_safe(String::from_utf8_lossy(&output.stdout).trim())
        }
        _ => "not found (git integration is not used in M0)".into(),
    }
}

#![deny(unsafe_code)]

mod approve;
mod auth;
mod config;
mod doctor;
mod prompt;
mod render;
mod sessions;
mod trust;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use raven_core::agent::{Agent, AgentConfig, Observer, RunSummary};
use raven_core::cost::CostTracker;
use raven_core::policy::{PathPolicy, Policy, PolicyMode};
use raven_core::redact::{sanitize_text, Redactor};
use raven_core::tool::Approver;
use raven_core::{Error, Result};
use raven_provider::OpenAiProvider;
use raven_store::Store;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::approve::ConsoleApprover;
use crate::render::ConsoleObserver;

#[derive(Debug, Parser)]
#[command(
    name = "raven",
    version,
    about = "Windows-first, provider-neutral coding agent (M1: exact edits with approval)",
    long_about = None
)]
struct Cli {
    /// Path to raven.toml (defaults to %APPDATA%\raven\raven.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

/// `Run` is boxed: it carries every run flag and would otherwise size the whole
/// enum up for the much smaller subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Run a read-only agent turn.
    Run(Box<RunArgs>),
    /// Write a sample configuration file.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Manage provider API keys in Windows Credential Manager.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Check configuration, credentials, and sandbox support.
    Doctor,
    /// List recent sessions.
    Sessions {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Manage trust for a project-local raven.toml.
    Trust {
        /// Workspace to act on. Defaults to the current directory.
        #[arg(long)]
        workspace: Option<std::path::PathBuf>,
        /// Trust the workspace after showing what its config would change.
        #[arg(long, conflicts_with = "revoke")]
        grant: bool,
        /// Remove trust; the project file stops being read.
        #[arg(long)]
        revoke: bool,
    },
    /// Print a session's event log.
    Show {
        session_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Prompt text. Everything after the flags is joined with spaces.
    #[arg(required = true, trailing_var_arg = true)]
    prompt: Vec<String>,

    #[arg(long)]
    workspace: Option<PathBuf>,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    /// Environment variable to read the API key from (overrides config).
    #[arg(long)]
    api_key_env: Option<String>,
    #[arg(long)]
    max_turns: Option<usize>,
    #[arg(long)]
    max_cost: Option<f64>,
    #[arg(long)]
    temperature: Option<f32>,
    #[arg(long)]
    max_tokens: Option<u32>,
    /// Record the raw provider responses to this cassette file for replay.
    #[arg(long, conflicts_with = "replay")]
    record: Option<PathBuf>,
    /// Replay from this cassette instead of calling the provider.
    #[arg(long)]
    replay: Option<PathBuf>,
    /// Refuse every file write for this run, whatever the config says.
    #[arg(long)]
    read_only: bool,
    /// Approve every file edit without prompting. Only use in trusted workspaces.
    #[arg(long, short = 'y', conflicts_with = "read_only")]
    yes: bool,
    /// Emit versioned JSONL events instead of human-readable output.
    #[arg(long)]
    json: bool,
    #[arg(long)]
    no_color: bool,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Store an API key for a provider.
    Set {
        provider: String,
        /// Read the key from stdin instead of prompting.
        #[arg(long)]
        stdin: bool,
    },
    /// Show which providers have credentials.
    Status,
    /// Delete a stored API key.
    Delete { provider: String },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli).await {
        Ok(code) => code,
        Err(Error::Cancelled) => {
            eprintln!("cancelled");
            ExitCode::from(130)
        }
        Err(error) => {
            eprintln!(
                "raven: {}",
                Redactor::default().redact(&sanitize_text(&error.to_string()))
            );
            ExitCode::from(error.exit_code())
        }
    }
}

async fn dispatch(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Run(args) => cmd_run(*args, cli.config).await,
        Command::Init { force } => {
            let path = config::init(cli.config, force)?;
            println!(
                "wrote {}",
                Redactor::default().redact(&sanitize_text(&path.display().to_string()))
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Auth { command } => {
            let cfg = config::Config::load(cli.config)?;
            match command {
                AuthCommand::Set { provider, stdin } => auth::set(&provider, stdin)?,
                AuthCommand::Status => auth::status(&cfg)?,
                AuthCommand::Delete { provider } => auth::delete(&provider)?,
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            doctor::run(cli.config)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Sessions { json, limit } => {
            sessions::list(&config::data_dir()?, limit, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Show { session_id, json } => {
            sessions::show(&config::data_dir()?, &session_id, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Trust {
            workspace,
            grant,
            revoke,
        } => trust::command(
            workspace.unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            grant,
            revoke,
        )
        .map(|()| ExitCode::SUCCESS),
    }
}

async fn cmd_run(args: RunArgs, config_path: Option<PathBuf>) -> Result<ExitCode> {
    let prompt = args.prompt.join(" ").trim().to_string();
    if prompt.is_empty() {
        return Err(Error::Config("prompt is empty".into()));
    }

    let workspace = match args.workspace {
        Some(path) => path,
        None => std::env::current_dir()?,
    };
    let workspace = dunce::canonicalize(&workspace).map_err(|error| {
        Error::Config(format!(
            "workspace '{}' is not accessible: {error}",
            workspace.display()
        ))
    })?;

    // The trust gate decides whether the project-local file is read at all.
    let (cfg, project_state) = config::Config::load_resolved(config_path, &workspace)?;

    let provider_name = args
        .provider
        .clone()
        .or_else(|| cfg.default_provider.clone())
        .unwrap_or_else(|| "openai".into());
    let resolved = cfg.resolve_provider(
        &provider_name,
        args.base_url.clone(),
        args.model.clone(),
        args.api_key_env.clone(),
    )?;
    let max_turns = args
        .max_turns
        .unwrap_or_else(|| cfg.effective_max_turns())
        .max(1);
    let max_cost = args.max_cost.or(cfg.max_cost_usd);
    let pricing = cfg.pricing.get(&resolved.model).copied();
    if max_cost.is_some() && pricing.is_none() {
        return Err(Error::Config(format!(
            "--max-cost requires a [pricing.\"{}\"] entry in raven.toml",
            resolved.model
        )));
    }

    let policy_mode = if args.read_only {
        PolicyMode::ReadOnly
    } else {
        cfg.effective_policy()
    };
    let policy = match policy_mode {
        PolicyMode::ReadOnly => Policy::read_only(),
        PolicyMode::WorkspaceWrite => Policy::workspace_write(),
    };

    let data_dir = config::data_dir()?;
    let store = Arc::new(Store::create(
        &data_dir,
        &workspace,
        &provider_name,
        &resolved.model,
        policy_mode.label(),
    )?);

    let path_policy = Arc::new(PathPolicy::new(&workspace)?);
    let redactor = Arc::new(Redactor::with_enabled(cfg.effective_redact()));
    let system_prompt = prompt::build(&workspace, &path_policy, &redactor, policy_mode)?;
    let tools = Arc::new(match policy_mode {
        PolicyMode::ReadOnly => raven_tools::read_only_registry(),
        PolicyMode::WorkspaceWrite => raven_tools::workspace_write_registry(),
    });

    // When recording, the cassette is written once the run ends; the recorder
    // outlives the provider so it must be kept alive here.
    let mut recorder: Option<Arc<raven_provider::Recorder>> = None;
    let provider: Arc<dyn raven_core::provider::Provider> = if let Some(path) = &args.replay {
        // Replays through the same SSE parser as the live path, so a cassette
        // exercises the real framing and tool-call assembly.
        let cassette = raven_provider::Cassette::load(path)?;
        Arc::new(raven_provider::CassetteProvider::new(cassette))
    } else {
        // Only live-provider execution resolves credentials; cassette replay
        // bypasses this branch entirely.
        let api_key = auth::resolve_key(&resolved, args.api_key_env.as_deref())?;
        let mut provider = OpenAiProvider::new(
            provider_name.clone(),
            resolved.base_url.clone(),
            resolved.model.clone(),
            api_key,
        )?
        .with_stream_usage(resolved.stream_usage);

        if let Some(path) = &args.record {
            let handle = Arc::new(raven_provider::Recorder::new(
                path.clone(),
                provider_name.clone(),
                resolved.model.clone(),
            ));
            provider = provider.with_recorder(handle.clone());
            recorder = Some(handle);
        }
        Arc::new(provider)
    };

    let observer = Arc::new(ConsoleObserver::new(
        args.json,
        !args.no_color,
        redactor.clone(),
    ));

    let approver: Arc<dyn Approver> =
        Arc::new(ConsoleApprover::new(args.yes, !args.no_color && !args.json));

    let cancel = CancellationToken::new();
    spawn_cancel_handler(cancel.clone());

    if args.json {
        render::emit(json!({
            "v": 1,
            "type": "session_started",
            "session_id": store.session_id(),
            "provider": provider_name,
            "model": resolved.model,
            "workspace": workspace.to_string_lossy(),
            "policy": policy_mode.label(),
        }));
    } else {
        let model_display = Redactor::default().redact(&sanitize_text(&resolved.model));
        let workspace_display =
            Redactor::default().redact(&sanitize_text(&workspace.to_string_lossy()));
        eprintln!(
            "{} · {} · {} · {}\n",
            store.session_id(),
            model_display,
            workspace_display,
            policy_mode.label()
        );
    }

    let agent_config = AgentConfig {
        max_turns,
        temperature: args.temperature.or(cfg.temperature),
        max_tokens: args.max_tokens.or(cfg.max_tokens),
        provider_name: provider_name.clone(),
        model: resolved.model.clone(),
    };

    // A trusted project config changed this run's behaviour; say so in the log
    // and in the JSON stream rather than applying it silently.
    if let Some(notice) = project_state.notice() {
        observer.on_notice(&notice);
        store.note(&notice, json!({ "project_config": project_state.label() }))?;
    }

    let mut agent = Agent::new(
        store.session_id().to_string(),
        system_prompt,
        provider,
        tools,
        store.clone(),
        observer,
        policy,
        redactor,
        path_policy,
        workspace.clone(),
        cfg.effective_output_cap(),
        agent_config,
        CostTracker::new(pricing, max_cost),
        cancel,
    )
    .with_approver(approver);

    let result = agent.run(prompt).await;

    // Recording writes the cassette after the run ends, however it ended: a
    // partial session is still worth replaying when debugging a failure.
    if let Some(recorder) = recorder {
        if let Err(error) = recorder.flush() {
            // Diagnostics belong on stderr in both modes; stdout stays a
            // machine-readable JSONL stream when --json is enabled.
            eprintln!("raven: cassette recording failed: {error}");
        }
    }

    match result {
        Ok(summary) => {
            store.finish("completed", summary.turns, summary.usage, summary.cost_usd)?;
            report_success(&summary, store.session_id(), args.json);
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            let status = match &error {
                Error::Cancelled => "cancelled",
                Error::Budget(_) => "budget_exceeded",
                Error::MaxTurns(_) => "max_turns",
                _ => "error",
            };
            let _ = store.finish(status, agent.turns(), agent.usage(), agent.cost_usd());
            if args.json {
                render::emit(json!({
                    "v": 1,
                    "type": "error",
                    "session_id": store.session_id(),
                    "message": error.to_string(),
                    "exit_code": error.exit_code(),
                }));
            }
            Err(error)
        }
    }
}

fn report_success(summary: &RunSummary, session_id: &str, json_output: bool) {
    if json_output {
        render::emit(json!({
            "v": 1,
            "type": "done",
            "session_id": session_id,
            "turns": summary.turns,
            "finish_reason": summary.finish_reason,
            "input_tokens": summary.usage.input_tokens,
            "output_tokens": summary.usage.output_tokens,
            "cached_input_tokens": summary.usage.cached_input_tokens,
            "cost_usd": if summary.cost_known { Some(summary.cost_usd) } else { None },
            "cost_known": summary.cost_known,
        }));
        return;
    }

    let cost = if summary.cost_known {
        format!("${:.4}", summary.cost_usd)
    } else {
        "cost unavailable (add a [pricing] entry)".to_string()
    };
    eprintln!(
        "\nsession {} · {} turns · {} in / {} out · {}",
        session_id, summary.turns, summary.usage.input_tokens, summary.usage.output_tokens, cost
    );
}

fn spawn_cancel_handler(cancel: CancellationToken) {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            cancel.cancel();
        }
        if tokio::signal::ctrl_c().await.is_ok() {
            std::process::exit(130);
        }
    });
}

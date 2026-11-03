use std::path::Path;

use raven_core::redact::truncate_utf8;
use raven_core::Result;
use raven_store::{list_sessions, read_events, read_meta, SessionMeta};

pub fn list(data_dir: &Path, limit: usize, json: bool) -> Result<()> {
    let sessions = list_sessions(data_dir, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        return Ok(());
    }

    if sessions.is_empty() {
        println!("no sessions yet");
        return Ok(());
    }

    println!(
        "{:<24} {:<20} {:<10} {:<16} {:>6} {:>10} {:>10}",
        "SESSION", "CREATED (UTC)", "STATUS", "MODEL", "TURNS", "TOKENS", "COST"
    );
    for session in sessions {
        let cost = if session.cost_usd > 0.0 {
            format!("${:.4}", session.cost_usd)
        } else {
            "-".into()
        };
        println!(
            "{:<24} {:<20} {:<10} {:<16} {:>6} {:>10} {:>10}",
            session.id,
            session.created_at.format("%Y-%m-%d %H:%M:%S"),
            session.status,
            truncate(&session.model, 16),
            session.turns,
            session.total_tokens(),
            cost
        );
    }
    Ok(())
}

pub fn show(data_dir: &Path, session_id: &str, json: bool) -> Result<()> {
    let events = read_events(data_dir, session_id)?;
    if json {
        for event in events {
            println!("{}", serde_json::to_string(&event)?);
        }
        return Ok(());
    }

    let meta = read_meta(data_dir, session_id)?;
    print_meta(&meta);
    println!();

    for event in events {
        println!(
            "#{:<4} [{}] {} {}",
            event.seq,
            event.kind,
            event.summary,
            event.ts.format("%H:%M:%S")
        );
        let data = serde_json::to_string_pretty(&event.data)?;
        let (data, truncated) = truncate_utf8(&data, 2000);
        for line in data.lines() {
            println!("      {line}");
        }
        if truncated {
            println!("      …");
        }
    }
    Ok(())
}

fn print_meta(meta: &SessionMeta) {
    println!("session:   {}", meta.id);
    println!("workspace: {}", meta.workspace);
    println!("provider:  {} ({})", meta.provider, meta.model);
    println!("policy:    {}", meta.policy);
    println!("status:    {} · {} turns", meta.status, meta.turns);
    println!(
        "usage:     {} in / {} out ({} cached)",
        meta.usage.input_tokens, meta.usage.output_tokens, meta.usage.cached_input_tokens
    );
    println!("cost:      ${:.4}", meta.cost_usd);
    println!("created:   {}", meta.created_at.to_rfc3339());
    println!("updated:   {}", meta.updated_at.to_rfc3339());
}

fn truncate(input: &str, max: usize) -> String {
    let (text, truncated) = truncate_utf8(input, max);
    if truncated {
        format!("{text}…")
    } else {
        text.to_string()
    }
}

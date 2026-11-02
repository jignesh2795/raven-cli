#![deny(unsafe_code)]

//! Append-only session log with a SHA-256 hash chain.
//!
//! M0 deliberately uses JSONL plus a `meta.json` file per session. A SQLite
//! projection/index is an M1 optimization; the log remains the source of truth
//! and must be replayable without the database.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use raven_core::agent::EventSink;
use raven_core::cost::Usage;
use raven_core::events::{verify_chain, Event, EventKind, GENESIS_HASH};
use raven_core::redact::Redactor;
use raven_core::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub workspace: String,
    pub provider: String,
    pub model: String,
    pub status: String,
    pub turns: usize,
    pub usage: Usage,
    pub cost_usd: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub raven_version: String,
}

impl SessionMeta {
    pub fn total_tokens(&self) -> u64 {
        self.usage.total_tokens()
    }
}

struct Inner {
    file: File,
    seq: u64,
    prev_hash: String,
}

pub struct Store {
    session_id: String,
    session_dir: PathBuf,
    meta_path: PathBuf,
    meta: Mutex<SessionMeta>,
    inner: Mutex<Inner>,
    redactor: Redactor,
}

impl Store {
    pub fn create(
        root: &Path,
        workspace: &Path,
        provider: &str,
        model: &str,
    ) -> Result<Self> {
        fs::create_dir_all(root.join("sessions"))?;

        let now = Utc::now();
        let id = format!(
            "{}-{}",
            now.format("%Y%m%d-%H%M%S"),
            &Uuid::new_v4().simple().to_string()[..6]
        );
        let session_dir = root.join("sessions").join(&id);
        fs::create_dir_all(&session_dir)?;
        let meta_path = session_dir.join("meta.json");
        let log_path = session_dir.join("events.jsonl");

        let meta = SessionMeta {
            id: id.clone(),
            workspace: workspace.to_string_lossy().to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            status: "running".into(),
            turns: 0,
            usage: Usage::default(),
            cost_usd: 0.0,
            created_at: now,
            updated_at: now,
            raven_version: SERVICE_VERSION.to_string(),
        };
        write_meta(&meta_path, &meta)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        let store = Self {
            session_id: id,
            session_dir,
            meta_path,
            meta: Mutex::new(meta),
            inner: Mutex::new(Inner {
                file,
                seq: 0,
                prev_hash: GENESIS_HASH.to_string(),
            }),
            redactor: Redactor::default(),
        };

        store.append(
            EventKind::SessionStarted,
            "session started",
            json!({
                "workspace": workspace.to_string_lossy(),
                "provider": provider,
                "model": model,
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "raven_version": SERVICE_VERSION,
            }),
        )?;

        Ok(store)
    }

    pub fn open(root: &Path, session_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let session_dir = root.join("sessions").join(session_id);
        let meta_path = session_dir.join("meta.json");
        let meta: SessionMeta = serde_json::from_slice(&fs::read(&meta_path)?)
            .map_err(|error| Error::Store(format!("invalid session metadata: {error}")))?;
        let events = read_events(root, session_id)?;
        let (seq, prev_hash) = events
            .last()
            .map(|event| (event.seq, event.hash.clone()))
            .unwrap_or((0, GENESIS_HASH.to_string()));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(session_dir.join("events.jsonl"))?;

        Ok(Self {
            session_id: session_id.to_string(),
            session_dir,
            meta_path,
            meta: Mutex::new(meta),
            inner: Mutex::new(Inner {
                file,
                seq,
                prev_hash,
            }),
            redactor: Redactor::default(),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn session_dir(&self) -> &Path {
        &self.session_dir
    }

    pub fn meta(&self) -> Result<SessionMeta> {
        self.meta
            .lock()
            .map(|meta| meta.clone())
            .map_err(|_| Error::Store("session metadata lock is poisoned".into()))
    }

    pub fn append(
        &self,
        kind: EventKind,
        summary: &str,
        data: Value,
    ) -> Result<Event> {
        let mut data = data;
        self.redactor.redact_json(&mut data);
        let summary = self.redactor.redact(summary);
        let summary: String = summary.chars().take(300).collect();

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| Error::Store("event log lock is poisoned".into()))?;

        let seq = inner.seq + 1;
        let event = Event::new(
            &self.session_id,
            seq,
            kind,
            summary,
            data,
            inner.prev_hash.clone(),
        );
        let line = serde_json::to_string(&event)?;
        writeln!(inner.file, "{line}")?;
        inner.file.flush()?;
        inner.seq = seq;
        inner.prev_hash = event.hash.clone();
        Ok(event)
    }

    pub fn finish(
        &self,
        status: &str,
        turns: usize,
        usage: Usage,
        cost_usd: f64,
    ) -> Result<()> {
        {
            let mut meta = self
                .meta
                .lock()
                .map_err(|_| Error::Store("session metadata lock is poisoned".into()))?;
            meta.status = status.to_string();
            meta.turns = turns;
            meta.usage = usage;
            meta.cost_usd = cost_usd;
            meta.updated_at = Utc::now();
            write_meta(&self.meta_path, &meta)?;
        }

        self.append(
            EventKind::SessionFinished,
            "session finished",
            json!({
                "status": status,
                "turns": turns,
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "cached_input_tokens": usage.cached_input_tokens,
                "cost_usd": cost_usd,
            }),
        )?;
        Ok(())
    }

    pub fn note(&self, summary: &str, data: Value) -> Result<()> {
        self.append(EventKind::Notice, summary, data).map(|_| ())
    }
}

impl EventSink for Store {
    fn append(&self, kind: EventKind, summary: &str, data: Value) -> Result<()> {
        Store::append(self, kind, summary, data).map(|_| ())
    }
}

pub fn read_events(root: &Path, session_id: &str) -> Result<Vec<Event>> {
    validate_session_id(session_id)?;
    let path = root
        .join("sessions")
        .join(session_id)
        .join("events.jsonl");
    let reader = BufReader::new(File::open(&path)?);
    let mut events = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: Event = serde_json::from_str(&line).map_err(|error| {
            Error::Store(format!(
                "invalid event at {}:{}: {error}",
                path.display(),
                index + 1
            ))
        })?;
        events.push(event);
    }
    verify_chain(&events).map_err(Error::Store)?;
    Ok(events)
}

pub fn read_meta(root: &Path, session_id: &str) -> Result<SessionMeta> {
    validate_session_id(session_id)?;
    let path = root.join("sessions").join(session_id).join("meta.json");
    serde_json::from_slice(&fs::read(&path)?)
        .map_err(|error| Error::Store(format!("invalid session metadata: {error}")))
}

pub fn list_sessions(root: &Path, limit: usize) -> Result<Vec<SessionMeta>> {
    let sessions_dir = root.join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in fs::read_dir(&sessions_dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let meta_path = entry.path().join("meta.json");
        let meta = match fs::read(&meta_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SessionMeta>(&bytes).ok())
        {
            Some(meta) => meta,
            None => continue,
        };
        sessions.push(meta);
    }

    sessions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    sessions.truncate(limit);
    Ok(sessions)
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id.contains("..")
        || session_id.contains('/')
        || session_id.contains('\\')
        || !session_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        return Err(Error::Store(format!(
            "invalid session id: {session_id:?}"
        )));
    }
    Ok(())
}

fn write_meta(path: &Path, meta: &SessionMeta) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(meta)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes)?;
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows can refuse to replace a destination opened by another
            // process. The retry is best-effort and still keeps the file valid.
            let _ = fs::remove_file(path);
            fs::rename(&temp, path)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_verifiable_chain_and_redacts() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::create(
            temp.path(),
            temp.path(),
            "test",
            "model",
        )
        .unwrap();
        store
            .append(
                EventKind::Notice,
                "secret test",
                json!({ "key": "sk-1234567890abcdefghijklmnop" }),
            )
            .unwrap();
        store
            .finish(
                "completed",
                1,
                Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    cached_input_tokens: 0,
                },
                0.01,
            )
            .unwrap();

        let events = read_events(temp.path(), store.session_id()).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events.last().unwrap().kind, EventKind::SessionFinished);

        let raw = fs::read_to_string(
            temp.path()
                .join("sessions")
                .join(store.session_id())
                .join("events.jsonl"),
        )
        .unwrap();
        assert!(!raw.contains("sk-1234567890"));

        let meta = read_meta(temp.path(), store.session_id()).unwrap();
        assert_eq!(meta.status, "completed");
        assert_eq!(meta.turns, 1);
        assert_eq!(meta.total_tokens(), 15);
    }

    #[test]
    fn lists_sessions_newest_first() {
        let temp = tempfile::tempdir().unwrap();
        let first = Store::create(temp.path(), temp.path(), "test", "model").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = Store::create(temp.path(), temp.path(), "test", "model").unwrap();

        let sessions = list_sessions(temp.path(), 10).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, second.session_id());
        assert_eq!(sessions[1].id, first.session_id());
    }
}

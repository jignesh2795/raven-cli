use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const EVENT_VERSION: u8 = 1;
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SessionStarted,
    UserMessage,
    ProviderRequest,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Usage,
    Notice,
    Error,
    Cancelled,
    SessionFinished,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStarted => "session_started",
            Self::UserMessage => "user_message",
            Self::ProviderRequest => "provider_request",
            Self::AssistantMessage => "assistant_message",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Usage => "usage",
            Self::Notice => "notice",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::SessionFinished => "session_finished",
        }
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One entry in the append-only, hash-chained session log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub v: u8,
    pub seq: u64,
    pub id: String,
    pub session_id: String,
    pub ts: DateTime<Utc>,
    pub kind: EventKind,
    pub summary: String,
    pub data: Value,
    pub prev_hash: String,
    pub hash: String,
}

#[derive(Serialize)]
struct HashInput<'a> {
    v: u8,
    seq: u64,
    id: &'a str,
    session_id: &'a str,
    ts: &'a DateTime<Utc>,
    kind: EventKind,
    summary: &'a str,
    data: &'a Value,
    prev_hash: &'a str,
}

impl Event {
    pub fn new(
        session_id: impl Into<String>,
        seq: u64,
        kind: EventKind,
        summary: impl Into<String>,
        data: Value,
        prev_hash: impl Into<String>,
    ) -> Self {
        let mut event = Self {
            v: EVENT_VERSION,
            seq,
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            ts: Utc::now(),
            kind,
            summary: summary.into(),
            data,
            prev_hash: prev_hash.into(),
            hash: String::new(),
        };
        event.hash = event.compute_hash();
        event
    }

    fn compute_hash(&self) -> String {
        let input = HashInput {
            v: self.v,
            seq: self.seq,
            id: &self.id,
            session_id: &self.session_id,
            ts: &self.ts,
            kind: self.kind,
            summary: &self.summary,
            data: &self.data,
            prev_hash: &self.prev_hash,
        };
        // Serialization of a struct containing a serde_json::Value cannot fail
        // for values produced by serde_json itself.
        let bytes = serde_json::to_vec(&input).expect("hash input is serializable");
        hex::encode(Sha256::digest(bytes))
    }

    pub fn is_valid(&self) -> bool {
        self.hash == self.compute_hash()
    }
}

/// Verify sequence numbers, previous-hash links, and each event hash.
pub fn verify_chain(events: &[Event]) -> std::result::Result<(), String> {
    let mut prev = GENESIS_HASH.to_string();
    for (index, event) in events.iter().enumerate() {
        let expected_seq = index as u64 + 1;
        if event.seq != expected_seq {
            return Err(format!(
                "event {}: expected seq {}, found {}",
                index, expected_seq, event.seq
            ));
        }
        if event.prev_hash != prev {
            return Err(format!("event {}: previous hash mismatch", index));
        }
        if !event.is_valid() {
            return Err(format!("event {}: hash mismatch (log tampered?)", index));
        }
        prev = event.hash.clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chain_round_trips_and_detects_tampering() {
        let mut events = Vec::new();
        let mut prev = GENESIS_HASH.to_string();
        for seq in 1..=3 {
            let event = Event::new(
                "session",
                seq,
                EventKind::Notice,
                format!("notice {seq}"),
                json!({"seq": seq}),
                prev,
            );
            prev = event.hash.clone();
            events.push(event);
        }
        verify_chain(&events).unwrap();

        events[1].summary = "tampered".into();
        assert!(verify_chain(&events).is_err());
    }
}

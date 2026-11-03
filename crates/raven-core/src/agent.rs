use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::cost::{CostTracker, Usage};
use crate::events::EventKind;
use crate::message::{Message, ToolCall};
use crate::policy::{PathPolicy, Policy};
use crate::provider::{ChatRequest, Provider, ProviderEvent};
use crate::redact::{sanitize_text, truncate_utf8, Redactor};
use crate::tool::{ToolContext, ToolRegistry};
use crate::{Error, Result};

/// Receives audit events. Implementations must be thread-safe because tools and
/// provider streams may run concurrently in later milestones.
pub trait EventSink: Send + Sync {
    fn append(&self, kind: EventKind, summary: &str, data: Value) -> Result<()>;
}

/// Receives streaming output for the terminal/JSON renderer. Callbacks are
/// intentionally synchronous and must not block for long.
pub trait Observer: Send + Sync {
    fn on_text(&self, _text: &str) {}
    fn on_tool_call(&self, _name: &str, _args: &Value) {}
    fn on_tool_result(&self, _name: &str, _ok: bool, _summary: &str) {}
    fn on_notice(&self, _message: &str) {}
}

#[derive(Debug, Default)]
pub struct NoopObserver;

impl Observer for NoopObserver {}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_turns: usize,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub provider_name: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct RunSummary {
    pub session_id: String,
    pub turns: usize,
    pub usage: Usage,
    pub cost_usd: f64,
    pub cost_known: bool,
    pub finish_reason: String,
}

pub struct Agent {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    sink: Arc<dyn EventSink>,
    observer: Arc<dyn Observer>,
    policy: Policy,
    redactor: Arc<Redactor>,
    path_policy: Arc<PathPolicy>,
    workspace: PathBuf,
    output_cap: usize,
    config: AgentConfig,
    cancel: CancellationToken,
    session_id: String,
    system_prompt: String,
    cost: CostTracker,
    turns: usize,
}

#[allow(clippy::too_many_arguments)]
impl Agent {
    pub fn new(
        session_id: String,
        system_prompt: String,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        sink: Arc<dyn EventSink>,
        observer: Arc<dyn Observer>,
        policy: Policy,
        redactor: Arc<Redactor>,
        path_policy: Arc<PathPolicy>,
        workspace: PathBuf,
        output_cap: usize,
        config: AgentConfig,
        cost: CostTracker,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            provider,
            tools,
            sink,
            observer,
            policy,
            redactor,
            path_policy,
            workspace,
            output_cap,
            config,
            cancel,
            session_id,
            system_prompt,
            cost,
            turns: 0,
        }
    }

    pub fn turns(&self) -> usize {
        self.turns
    }

    pub fn usage(&self) -> Usage {
        self.cost.usage
    }

    pub fn cost_usd(&self) -> f64 {
        self.cost.cost_usd
    }

    pub async fn run(&mut self, user_input: String) -> Result<RunSummary> {
        let mut messages = vec![
            Message::system(self.system_prompt.clone()),
            Message::user(user_input.clone()),
        ];

        self.sink.append(
            EventKind::UserMessage,
            "user prompt",
            json!({ "text": user_input }),
        )?;

        let mut finish_reason = String::from("stop");

        for turn in 1..=self.config.max_turns {
            self.turns = turn;
            if self.cancel.is_cancelled() {
                self.sink
                    .append(EventKind::Cancelled, "cancelled by user", json!({}))?;
                return Err(Error::Cancelled);
            }

            self.sink.append(
                EventKind::ProviderRequest,
                format!(
                    "request to {} ({})",
                    self.provider.name(),
                    self.provider.model()
                )
                .as_str(),
                json!({
                    "provider": self.config.provider_name,
                    "model": self.config.model,
                    "message_count": messages.len(),
                    "tools": self.tools.names(),
                }),
            )?;

            let request = ChatRequest {
                messages: messages.clone(),
                tools: self.tools.specs(),
                temperature: self.config.temperature,
                max_tokens: self.config.max_tokens,
            };

            let mut stream = self
                .provider
                .stream_chat(request, self.cancel.clone())
                .await?;

            let mut text = String::new();
            let mut pending: BTreeMap<usize, PendingToolCall> = BTreeMap::new();
            let mut turn_finish: Option<String> = None;

            while let Some(item) = stream.next().await {
                if self.cancel.is_cancelled() {
                    self.sink
                        .append(EventKind::Cancelled, "cancelled by user", json!({}))?;
                    return Err(Error::Cancelled);
                }

                match item {
                    Ok(ProviderEvent::TextDelta(delta)) => {
                        text.push_str(&delta);
                        self.observer.on_text(&delta);
                    }
                    Ok(ProviderEvent::ToolCallDelta {
                        index,
                        id,
                        name,
                        arguments,
                    }) => {
                        let pending_call = pending.entry(index).or_default();
                        if let Some(id) = id {
                            if !id.is_empty() {
                                pending_call.id = id;
                            }
                        }
                        if let Some(name) = name {
                            if !name.is_empty() {
                                pending_call.name.push_str(&name);
                            }
                        }
                        if let Some(arguments) = arguments {
                            pending_call.arguments.push_str(&arguments);
                        }
                    }
                    Ok(ProviderEvent::Usage(usage)) => {
                        self.sink.append(
                            EventKind::Usage,
                            "token usage",
                            json!({
                                "turn": turn,
                                "input_tokens": usage.input_tokens,
                                "output_tokens": usage.output_tokens,
                                "cached_input_tokens": usage.cached_input_tokens,
                            }),
                        )?;
                        if let Err(error) = self.cost.record(usage) {
                            self.sink.append(
                                EventKind::Error,
                                "cost budget exceeded",
                                json!({ "error": error.to_string() }),
                            )?;
                            return Err(error);
                        }
                    }
                    Ok(ProviderEvent::Done {
                        finish_reason: reason,
                    }) => {
                        turn_finish = reason;
                        break;
                    }
                    Err(error) => {
                        let (kind, summary) = if matches!(&error, Error::Cancelled) {
                            (EventKind::Cancelled, "cancelled by user")
                        } else {
                            (EventKind::Error, "provider stream failed")
                        };
                        self.sink
                            .append(kind, summary, json!({ "error": error.to_string() }))?;
                        return Err(error);
                    }
                }
            }

            if self.cancel.is_cancelled() {
                self.sink
                    .append(EventKind::Cancelled, "cancelled by user", json!({}))?;
                return Err(Error::Cancelled);
            }

            let tool_calls = build_tool_calls(pending, turn);
            let assistant_text = if text.is_empty() {
                None
            } else {
                Some(text.clone())
            };

            self.sink.append(
                EventKind::AssistantMessage,
                "assistant response",
                json!({
                    "text": assistant_text,
                    "tool_calls": tool_calls.iter().map(|call| json!({
                        "id": call.id,
                        "name": call.name,
                        "arguments": call.arguments,
                    })).collect::<Vec<_>>(),
                }),
            )?;

            messages.push(Message::Assistant {
                content: assistant_text,
                tool_calls: tool_calls.clone(),
            });

            if text.trim().is_empty() && tool_calls.is_empty() {
                let error = Error::Provider("model returned an empty response".into());
                self.sink.append(
                    EventKind::Error,
                    "empty response",
                    json!({ "error": error.to_string() }),
                )?;
                return Err(error);
            }

            if tool_calls.is_empty() {
                if let Some(reason) = turn_finish {
                    finish_reason = reason;
                }
                if finish_reason == "length" {
                    self.observer
                        .on_notice("response stopped at the model token limit");
                }
                return Ok(RunSummary {
                    session_id: self.session_id.clone(),
                    turns: turn,
                    usage: self.cost.usage,
                    cost_usd: self.cost.cost_usd,
                    cost_known: self.cost.cost_known(),
                    finish_reason,
                });
            }

            for call in tool_calls {
                if self.cancel.is_cancelled() {
                    self.sink
                        .append(EventKind::Cancelled, "cancelled by user", json!({}))?;
                    return Err(Error::Cancelled);
                }

                self.sink.append(
                    EventKind::ToolCall,
                    format!("tool {}", call.name).as_str(),
                    json!({
                        "id": call.id,
                        "name": call.name,
                        "arguments": call.arguments,
                    }),
                )?;

                let args = match parse_arguments(&call.arguments) {
                    Ok(args) => args,
                    Err(error) => {
                        let content = sanitize_text(&format!("ERROR: {error}"));
                        self.observer.on_tool_result(&call.name, false, &content);
                        self.sink.append(
                            EventKind::ToolResult,
                            format!("tool {} failed", call.name).as_str(),
                            json!({
                                "id": call.id,
                                "name": call.name,
                                "ok": false,
                                "content": content,
                                "truncated": false,
                            }),
                        )?;
                        messages.push(Message::Tool {
                            tool_call_id: call.id,
                            content,
                        });
                        continue;
                    }
                };

                self.observer.on_tool_call(&call.name, &args);

                let Some(tool) = self.tools.get(&call.name).cloned() else {
                    let content = sanitize_text(&format!("ERROR: unknown tool '{}'", call.name));
                    self.observer.on_tool_result(&call.name, false, &content);
                    self.sink.append(
                        EventKind::ToolResult,
                        format!("tool {} failed", call.name).as_str(),
                        json!({
                            "id": call.id,
                            "name": call.name,
                            "ok": false,
                            "content": content,
                            "truncated": false,
                        }),
                    )?;
                    messages.push(Message::Tool {
                        tool_call_id: call.id,
                        content,
                    });
                    continue;
                };

                let (content, ok, truncated) =
                    if let Err(error) = self.policy.check_tool(tool.kind()) {
                        (format!("ERROR: {error}"), false, false)
                    } else {
                        let context = ToolContext {
                            workspace: self.workspace.clone(),
                            path_policy: self.path_policy.clone(),
                            redactor: self.redactor.clone(),
                            output_cap: self.output_cap,
                        };
                        match tool.call(args, &context).await {
                            Ok(output) => {
                                let redacted = self.redactor.redact(&output.content);
                                let (capped, extra) = truncate_utf8(&redacted, self.output_cap);
                                (capped.to_string(), true, output.truncated || extra)
                            }
                            Err(error) => (format!("ERROR: {error}"), false, false),
                        }
                    };
                let content = sanitize_text(&content);

                let summary = summarize(&content);
                self.observer.on_tool_result(&call.name, ok, &summary);
                self.sink.append(
                    EventKind::ToolResult,
                    format!(
                        "tool {} {}",
                        call.name,
                        if ok { "completed" } else { "failed" }
                    )
                    .as_str(),
                    json!({
                        "id": call.id,
                        "name": call.name,
                        "ok": ok,
                        "content": content,
                        "truncated": truncated,
                    }),
                )?;

                messages.push(Message::Tool {
                    tool_call_id: call.id,
                    content,
                });
            }

            if turn == self.config.max_turns {
                self.sink.append(
                    EventKind::Error,
                    "maximum turns reached",
                    json!({ "max_turns": self.config.max_turns }),
                )?;
                return Err(Error::MaxTurns(self.config.max_turns));
            }
        }

        Err(Error::MaxTurns(self.config.max_turns))
    }
}

#[derive(Default)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn build_tool_calls(pending: BTreeMap<usize, PendingToolCall>, turn: usize) -> Vec<ToolCall> {
    pending
        .into_iter()
        .filter(|(_, call)| !call.name.is_empty())
        .map(|(index, mut call)| {
            if call.id.is_empty() {
                call.id = format!("call_{turn}_{index}");
            }
            if call.arguments.trim().is_empty() {
                call.arguments = "{}".into();
            }
            ToolCall {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
            }
        })
        .collect()
}

fn parse_arguments(raw: &str) -> Result<Value> {
    if raw.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(raw)
        .map_err(|error| Error::Tool(format!("tool arguments are not valid JSON: {error}")))
}

fn summarize(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").trim();
    let (capped, _) = truncate_utf8(first_line, 200);
    if content.lines().count() > 1 {
        format!("{capped} …")
    } else {
        capped.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderStream;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemorySink {
        events: Mutex<Vec<(EventKind, String)>>,
    }

    impl EventSink for MemorySink {
        fn append(&self, kind: EventKind, summary: &str, _data: Value) -> Result<()> {
            self.events
                .lock()
                .map_err(|_| Error::Store("test sink is poisoned".into()))?
                .push((kind, summary.to_string()));
            Ok(())
        }
    }

    struct SlowProvider;

    #[async_trait]
    impl Provider for SlowProvider {
        fn name(&self) -> &str {
            "test"
        }

        fn model(&self) -> &str {
            "test-model"
        }

        async fn stream_chat(
            &self,
            _request: ChatRequest,
            cancel: CancellationToken,
        ) -> Result<ProviderStream> {
            let stream = async_stream::stream! {
                yield Ok(ProviderEvent::TextDelta("hello".into()));
                cancel.cancelled().await;
                yield Err(Error::Cancelled);
            };
            Ok(Box::pin(stream))
        }
    }

    #[tokio::test]
    async fn cancellation_stops_an_in_flight_stream() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().to_path_buf();
        let sink = Arc::new(MemorySink::default());
        let cancel = CancellationToken::new();

        let mut agent = Agent::new(
            "test-session".into(),
            "system".into(),
            Arc::new(SlowProvider),
            Arc::new(ToolRegistry::new()),
            sink.clone(),
            Arc::new(NoopObserver),
            Policy::read_only(),
            Arc::new(Redactor::default()),
            Arc::new(PathPolicy::new(&workspace).unwrap()),
            workspace,
            64 * 1024,
            AgentConfig {
                max_turns: 3,
                temperature: None,
                max_tokens: None,
                provider_name: "test".into(),
                model: "test-model".into(),
            },
            CostTracker::new(None, None),
            cancel.clone(),
        );

        let handle = tokio::spawn(async move { agent.run("hello".into()).await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        cancel.cancel();

        let error = handle.await.unwrap().unwrap_err();
        assert!(matches!(error, Error::Cancelled));

        let events = sink.events.lock().unwrap();
        assert!(events.iter().any(|(kind, _)| *kind == EventKind::Cancelled));
    }
}

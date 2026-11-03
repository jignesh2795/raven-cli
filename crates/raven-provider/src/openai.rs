use std::time::Duration;

use async_stream::try_stream;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use raven_core::cost::Usage;
use raven_core::message::{Message, ToolSpec};
use raven_core::provider::{ChatRequest, Provider, ProviderEvent, ProviderStream};
use raven_core::{Error, Result};
use reqwest::{Client, Response, StatusCode};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_ATTEMPTS: u32 = 3;

pub struct OpenAiProvider {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    name: String,
    stream_usage: bool,
}

impl OpenAiProvider {
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Result<Self> {
        let base_url = base_url.into();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(Error::Config(format!(
                "provider base URL must start with http:// or https://: {base_url}"
            )));
        }

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .user_agent(format!("raven-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Error::Provider(format!("could not create HTTP client: {error}")))?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model: model.into(),
            name: name.into(),
            stream_usage: true,
        })
    }

    pub fn with_stream_usage(mut self, stream_usage: bool) -> Self {
        self.stream_usage = stream_usage;
        self
    }

    fn endpoint(&self) -> String {
        if self.base_url.ends_with("/chat/completions") {
            self.base_url.clone()
        } else {
            format!("{}/chat/completions", self.base_url)
        }
    }

    fn request_body(&self, request: &ChatRequest) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": messages_json(&request.messages),
            "stream": true,
        });

        if self.stream_usage {
            body["stream_options"] = json!({ "include_usage": true });
        }
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(request.tools.iter().map(tool_json).collect::<Vec<_>>());
        }
        if let Some(temperature) = request.temperature {
            body["temperature"] = json!(temperature);
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        body
    }

    async fn send_with_retry(&self, body: &Value) -> Result<Response> {
        let mut last_error: Option<Error> = None;

        for attempt in 0..MAX_ATTEMPTS {
            let mut request = self.client.post(self.endpoint()).json(body);
            if let Some(api_key) = &self.api_key {
                request = request.bearer_auth(api_key);
            }

            match request.send().await {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) if is_retryable(response.status()) => {
                    let status = response.status();
                    let retry_after = retry_after_seconds(&response);
                    let error = self.status_error(response).await;
                    last_error = Some(error);
                    if attempt + 1 == MAX_ATTEMPTS {
                        break;
                    }
                    let delay = retry_after.unwrap_or_else(|| {
                        let base = 0.5 * 2_f64.powi(attempt as i32);
                        base.min(8.0)
                    });
                    tokio::time::sleep(Duration::from_secs_f64(delay)).await;
                    let _ = status;
                }
                Ok(response) => return Err(self.status_error(response).await),
                Err(error) if error.is_connect() || error.is_timeout() || error.is_request() => {
                    last_error = Some(Error::Provider(format!(
                        "{} request failed: {error}",
                        self.name
                    )));
                    if attempt + 1 == MAX_ATTEMPTS {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs_f64(0.5 * 2_f64.powi(attempt as i32)))
                        .await;
                }
                Err(error) => {
                    return Err(Error::Provider(format!(
                        "{} request failed: {error}",
                        self.name
                    )))
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Provider(format!("{} request failed after retries", self.name))
        }))
    }

    async fn status_error(&self, response: Response) -> Error {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let snippet = truncate(&body, 1200);
        match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Error::Provider(format!(
                "{} rejected the API key (HTTP {}). Run `raven auth set {}` or check api_key_env. {}",
                self.name,
                status.as_u16(),
                self.name,
                snippet
            )),
            StatusCode::NOT_FOUND => Error::Provider(format!(
                "{} endpoint not found (HTTP 404). Check base_url '{}' and model '{}'. {}",
                self.name, self.base_url, self.model, snippet
            )),
            StatusCode::TOO_MANY_REQUESTS => Error::Provider(format!(
                "{} rate-limited the request (HTTP 429). {}",
                self.name, snippet
            )),
            _ => Error::Provider(format!(
                "{} returned HTTP {}: {}",
                self.name,
                status.as_u16(),
                snippet
            )),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream> {
        let body = self.request_body(&request);
        let response = self.send_with_retry(&body).await?;
        let mut source = response.bytes_stream().eventsource();

        let model = self.model.clone();
        let provider_name = self.name.clone();

        let stream = try_stream! {
            let mut finish_reason: Option<String> = None;
            let mut done = false;

            loop {
                let next = tokio::select! {
                    _ = cancel.cancelled() => None,
                    result = tokio::time::timeout(STREAM_IDLE_TIMEOUT, source.next()) => Some(result),
                };

                let event = match next {
                    None => {
                        Err(Error::Cancelled)?;
                        unreachable!();
                    }
                    Some(Err(_)) => {
                        Err(Error::Provider(format!(
                            "{provider_name} stream timed out after {}s",
                            STREAM_IDLE_TIMEOUT.as_secs()
                        )))?;
                        unreachable!();
                    }
                    Some(Ok(None)) => break,
                    Some(Ok(Some(Err(error)))) => {
                        Err(Error::Provider(format!("{provider_name} stream error: {error}")))?;
                        unreachable!();
                    }
                    Some(Ok(Some(Ok(event)))) => event,
                };

                let data = event.data;
                if data.trim().is_empty() {
                    continue;
                }
                if data.trim() == "[DONE]" {
                    yield ProviderEvent::Done { finish_reason: finish_reason.take() };
                    done = true;
                    break;
                }

                let value: Value = serde_json::from_str(&data).map_err(|error| {
                    Error::Provider(format!(
                        "{provider_name} sent invalid stream JSON: {error}: {}",
                        truncate(&data, 500)
                    ))
                })?;

                let parsed = parse_chunk(&value, &model, &provider_name)?;
                if let Some(reason) = parsed.finish_reason {
                    finish_reason = Some(reason);
                }
                for item in parsed.events {
                    yield item;
                }
            }

            if !done {
                yield ProviderEvent::Done { finish_reason };
            }
        };

        Ok(Box::pin(stream))
    }
}

struct ParsedChunk {
    events: Vec<ProviderEvent>,
    finish_reason: Option<String>,
}

fn parse_chunk(value: &Value, model: &str, provider_name: &str) -> Result<ParsedChunk> {
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("unknown provider error");
        return Err(Error::Provider(format!(
            "{provider_name} returned an error for model {model}: {message}"
        )));
    }

    let mut events = Vec::new();
    let mut finish_reason = None;

    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(text) = delta.get("content").and_then(content_text) {
                    if !text.is_empty() {
                        events.push(ProviderEvent::TextDelta(text));
                    }
                }
                if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for (fallback_index, tool_call) in tool_calls.iter().enumerate() {
                        let index = tool_call
                            .get("index")
                            .and_then(Value::as_u64)
                            .unwrap_or(fallback_index as u64)
                            as usize;
                        let id = tool_call
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let function = tool_call.get("function");
                        let name = function
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let arguments = function
                            .and_then(|function| function.get("arguments"))
                            .and_then(|arguments| match arguments {
                                Value::String(text) => Some(text.clone()),
                                Value::Object(_) => Some(arguments.to_string()),
                                _ => None,
                            });

                        if id.is_some() || name.is_some() || arguments.is_some() {
                            events.push(ProviderEvent::ToolCallDelta {
                                index,
                                id,
                                name,
                                arguments,
                            });
                        }
                    }
                }
            }

            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                if !reason.is_empty() && reason != "null" {
                    finish_reason = Some(reason.to_string());
                }
            }
        }
    }

    if let Some(usage) = value.get("usage").and_then(parse_usage) {
        events.push(ProviderEvent::Usage(usage));
    }

    Ok(ParsedChunk {
        events,
        finish_reason,
    })
}

fn messages_json(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| match message {
            Message::System { content } => json!({
                "role": "system",
                "content": content,
            }),
            Message::User { content } => json!({
                "role": "user",
                "content": content,
            }),
            Message::Assistant {
                content,
                tool_calls,
            } => {
                let mut value = json!({
                    "role": "assistant",
                    "content": content,
                });
                if !tool_calls.is_empty() {
                    value["tool_calls"] = Value::Array(
                        tool_calls
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.id,
                                    "type": "function",
                                    "function": {
                                        "name": call.name,
                                        "arguments": call.arguments,
                                    },
                                })
                            })
                            .collect(),
                    );
                }
                value
            }
            Message::Tool {
                tool_call_id,
                content,
            } => json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": content,
            }),
        })
        .collect()
}

fn tool_json(spec: &ToolSpec) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": spec.parameters,
        },
    })
}

fn content_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let mut output = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    output.push_str(text);
                }
            }
            if output.is_empty() {
                None
            } else {
                Some(output)
            }
        }
        _ => None,
    }
}

fn parse_usage(value: &Value) -> Option<Usage> {
    let input_tokens = value
        .get("prompt_tokens")
        .or_else(|| value.get("input_tokens"))
        .and_then(Value::as_u64)?;
    let output_tokens = value
        .get("completion_tokens")
        .or_else(|| value.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached_input_tokens = value
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    Some(Usage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
    })
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn retry_after_seconds(response: &Response) -> Option<f64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .map(|seconds| seconds.clamp(0.5, 30.0))
}

fn truncate(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    let mut end = max;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &input[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_finish_reason() {
        let value = json!({
            "choices": [{
                "delta": { "content": "hello" },
                "finish_reason": "stop"
            }]
        });
        let parsed = parse_chunk(&value, "model", "test").unwrap();
        assert_eq!(parsed.events.len(), 1);
        assert!(matches!(&parsed.events[0], ProviderEvent::TextDelta(text) if text == "hello"));
        assert_eq!(parsed.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn assembles_fragmented_tool_calls() {
        let first = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": { "name": "read_file", "arguments": "{\"path\":" }
                    }]
                }
            }]
        });
        let second = json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "\"Cargo.toml\"}" }
                    }]
                }
            }]
        });

        let parsed = parse_chunk(&first, "model", "test").unwrap();
        let ProviderEvent::ToolCallDelta {
            id,
            name,
            arguments,
            ..
        } = &parsed.events[0]
        else {
            panic!("expected tool call delta");
        };
        assert_eq!(id.as_deref(), Some("call_1"));
        assert_eq!(name.as_deref(), Some("read_file"));
        assert_eq!(arguments.as_deref(), Some("{\"path\":"));

        let parsed = parse_chunk(&second, "model", "test").unwrap();
        let ProviderEvent::ToolCallDelta { arguments, .. } = &parsed.events[0] else {
            panic!("expected tool call delta");
        };
        assert_eq!(arguments.as_deref(), Some("\"Cargo.toml\"}"));
    }

    #[test]
    fn parses_usage_and_cache_details() {
        let value = json!({
            "choices": [],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_tokens_details": { "cached_tokens": 60 }
            }
        });
        let parsed = parse_chunk(&value, "model", "test").unwrap();
        let ProviderEvent::Usage(usage) = &parsed.events[0] else {
            panic!("expected usage");
        };
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.cached_input_tokens, 60);
    }

    #[test]
    fn surfaces_provider_errors() {
        let value = json!({ "error": { "message": "bad key" } });
        assert!(parse_chunk(&value, "model", "test").is_err());
    }
}

//! CLI-level regressions for the audit fixes (H1-H3).
//!
//! These tests invoke the built `raven` binary, use a loopback mock provider,
//! and isolate all state in temporary directories. No real credentials or
//! external services are used.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use raven_core::events::{Event, EventKind, GENESIS_HASH};
use serde_json::Value;

const SSE_BODY: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: [DONE]\n\n",
);

struct MockProvider {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
}

impl MockProvider {
    fn start(expected_requests: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback mock provider");
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);

        thread::spawn(move || {
            for _ in 0..expected_requests {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                handle_request(stream, &captured);
            }
        });

        Self { port, requests }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

fn handle_request(mut stream: TcpStream, requests: &Arc<Mutex<Vec<String>>>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set mock read timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("clone mock stream"));
    let mut content_length = 0usize;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0u8; content_length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    requests
        .lock()
        .unwrap()
        .push(String::from_utf8_lossy(&body).into_owned());

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        SSE_BODY.len(),
        SSE_BODY
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn raven() -> &'static str {
    env!("CARGO_BIN_EXE_raven")
}

fn command(config: &Path, data: &Path) -> Command {
    let mut command = Command::new(raven());
    command
        .arg("--config")
        .arg(config)
        .env("RAVEN_DATA_DIR", data)
        .env_remove("RAVEN_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("NVIDIA_API_KEY");
    command
}

fn write_config(path: &Path, provider: &str, base_url: &str, model: &str) {
    std::fs::write(
        path,
        format!(
            "default_provider = \"{provider}\"\nmax_turns = 3\npolicy = \"read-only\"\n\n[providers.{provider}]\nbase_url = \"{base_url}\"\nmodel = \"{model}\"\n"
        ),
    )
    .unwrap();
}

fn workspace(temp: &Path) -> PathBuf {
    let workspace = temp.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    workspace
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_jsonl_with_version(stdout: &str) -> Vec<Value> {
    let mut values = Vec::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("stdout line is not JSON: {line:?}: {error}"));
        assert_eq!(
            value.get("v").and_then(Value::as_u64),
            Some(1),
            "stdout event is missing the documented version: {value}"
        );
        values.push(value);
    }
    values
}

#[test]
fn replay_works_without_an_api_key_and_does_not_contact_the_provider() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace(temp.path());
    let data = temp.path().join("data");
    let cassette = temp.path().join("recorded.jsonl");
    let mock = MockProvider::start(1);

    let live_config = temp.path().join("live.toml");
    write_config(&live_config, "mock", &mock.base_url(), "mock-model");

    let recorded = command(&live_config, &data)
        .arg("run")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--record")
        .arg(&cassette)
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&recorded, "record run");
    assert!(cassette.exists(), "record run did not write a cassette");
    assert_eq!(
        mock.request_count(),
        1,
        "record run should make one request"
    );

    // A remote, unreachable endpoint and no key: replay must not need either.
    let replay_config = temp.path().join("replay.toml");
    write_config(
        &replay_config,
        "replay",
        "https://example.invalid/v1",
        "replay-model",
    );

    let replayed = command(&replay_config, &data)
        .arg("run")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--replay")
        .arg(&cassette)
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&replayed, "replay run");
    assert!(
        String::from_utf8_lossy(&replayed.stdout).contains("ok"),
        "replay output did not contain the recorded response text"
    );
    assert_eq!(
        mock.request_count(),
        1,
        "replay contacted the configured remote provider"
    );
}

#[test]
fn live_provider_still_requires_an_api_key() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace(temp.path());
    let data = temp.path().join("data");
    let config = temp.path().join("remote.toml");
    let provider = format!("validation-no-key-{}", std::process::id());
    write_config(
        &config,
        &provider,
        "https://example.invalid/v1",
        "remote-model",
    );

    let output = command(&config, &data)
        .arg("run")
        .arg("--workspace")
        .arg(&workspace)
        .arg("hello")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "live provider run unexpectedly succeeded without a key"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no API key"),
        "expected a missing-key error, got: {stderr}"
    );
}

#[test]
fn project_pricing_cannot_satisfy_the_max_cost_requirement() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace(temp.path());
    let data = temp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();

    let user_config = temp.path().join("user.toml");
    write_config(
        &user_config,
        "mock",
        "https://example.invalid/v1",
        "user-model",
    );

    let project =
        "[pricing.\"project-model\"]\ninput_per_mtok = 0.0\noutput_per_mtok = 0.0\ncached_input_per_mtok = 0.0\n";
    std::fs::write(workspace.join("raven.toml"), project).unwrap();
    let trust = serde_json::json!([{
        "workspace": dunce::canonicalize(&workspace)
            .unwrap()
            .to_string_lossy()
            .to_lowercase(),
        "config_sha256": raven_core::util::sha256_hex(project.as_bytes()),
        "trusted_at": "2026-01-01T00:00:00Z",
    }]);
    std::fs::write(data.join("trust.json"), serde_json::to_vec(&trust).unwrap()).unwrap();

    let output = command(&user_config, &data)
        .arg("run")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--model")
        .arg("project-model")
        .arg("--max-cost")
        .arg("1")
        .arg("hello")
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "run unexpectedly accepted project-supplied pricing"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires a [pricing.\"project-model\"] entry"),
        "project pricing leaked into the max-cost check: {stderr}"
    );
    assert!(
        !stderr.contains("no API key"),
        "the pricing check should fail before authentication: {stderr}"
    );
}

#[test]
fn json_record_keeps_stdout_pure_jsonl_and_reports_success_on_stderr() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace(temp.path());
    let data = temp.path().join("data");
    let cassette = temp.path().join("recorded.jsonl");
    let mock = MockProvider::start(1);
    let config = temp.path().join("local.toml");
    write_config(&config, "mock", &mock.base_url(), "mock-model");

    let output = command(&config, &data)
        .arg("run")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--json")
        .arg("--record")
        .arg(&cassette)
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&output, "json record run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("recorded "),
        "recorder success text leaked to stdout: {stdout}"
    );
    let events = assert_jsonl_with_version(&stdout);
    assert!(
        events
            .iter()
            .any(|event| event.get("type").and_then(Value::as_str) == Some("done")),
        "JSON stream did not contain a done event: {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("recorded 1 response(s)"),
        "recorder success message missing from stderr: {stderr}"
    );
}

#[test]
fn json_record_failure_is_visible_on_stderr() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace(temp.path());
    let data = temp.path().join("data");
    let cassette_dir = temp.path().join("cassette-dir");
    std::fs::create_dir_all(&cassette_dir).unwrap();
    let mock = MockProvider::start(1);
    let config = temp.path().join("local.toml");
    write_config(&config, "mock", &mock.base_url(), "mock-model");

    let output = command(&config, &data)
        .arg("run")
        .arg("--workspace")
        .arg(&workspace)
        .arg("--json")
        .arg("--record")
        .arg(&cassette_dir)
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&output, "json record failure run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let events = assert_jsonl_with_version(&stdout);
    assert!(
        events
            .iter()
            .any(|event| event.get("type").and_then(Value::as_str) == Some("done")),
        "JSON stream did not contain a done event: {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cassette recording failed"),
        "recorder failure was not visible on stderr: {stderr}"
    );
}

#[test]
fn human_readable_output_still_works() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace(temp.path());
    let data = temp.path().join("data");
    let mock = MockProvider::start(1);
    let config = temp.path().join("local.toml");
    write_config(&config, "mock", &mock.base_url(), "mock-model");

    let output = command(&config, &data)
        .arg("run")
        .arg("--workspace")
        .arg(&workspace)
        .arg("hello")
        .output()
        .unwrap();
    assert_success(&output, "human-readable run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ok"),
        "human-readable output did not contain the response: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("session "),
        "human-readable session summary missing from stderr: {stderr}"
    );
}

#[test]
fn trust_review_sanitizes_untrusted_project_config_output() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = workspace(temp.path());
    let data = temp.path().join("data");
    let project = r#"
default_provider = "local"

[providers.local]
base_url = "https://user:password123@evil.example/\u001b]0;owned\u0007?token=abcdefghijklmnopqrst123456"
model = "mock\u001b[31mMODEL\u001b[0m"
api_key_env = "AWS_SECRET_ACCESS_KEY"
"#;
    std::fs::write(workspace.join("raven.toml"), project).unwrap();

    let output = Command::new(raven())
        .arg("trust")
        .arg("--workspace")
        .arg(&workspace)
        .env("RAVEN_DATA_DIR", &data)
        .output()
        .unwrap();
    assert_success(&output, "trust review with untrusted terminal content");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "untrusted project config emitted a terminal escape: {stdout:?}"
    );
    assert!(
        !stdout.contains("password123") && !stdout.contains("user:"),
        "URL userinfo leaked in the trust review: {stdout}"
    );
    assert!(
        !stdout.contains("token="),
        "URL query parameters were displayed in the trust review: {stdout}"
    );
    assert!(
        stdout.contains("[REDACTED]"),
        "URL credentials were not visibly redacted: {stdout}"
    );
    assert!(
        stdout.contains("mockMODEL"),
        "the model display should remain readable after sanitization: {stdout}"
    );
}

#[test]
fn doctor_sanitizes_provider_config_before_terminal_display() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let config = temp.path().join("doctor.toml");
    std::fs::write(
        &config,
        r#"
default_provider = "mock"
[providers."mock\u001b[31mBAD"]
base_url = "https://example.invalid/\u001b]0;owned\u0007"
model = "model\u001b[31mBAD"
api_key_env = "UNUSED_TEST_API_KEY"
"#,
    )
    .unwrap();

    let output = Command::new(raven())
        .arg("--config")
        .arg(&config)
        .arg("doctor")
        .env("RAVEN_DATA_DIR", &data)
        .output()
        .unwrap();
    assert_success(&output, "doctor with terminal-control provider config");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains('\u{1b}'),
        "doctor emitted a terminal escape from provider config: {stdout:?}"
    );
    assert!(
        stdout.contains("mockBAD"),
        "provider name should remain readable after sanitization: {stdout}"
    );
    assert!(
        stdout.contains("example.invalid"),
        "provider endpoint should remain readable after sanitization: {stdout}"
    );
}

#[test]
fn show_and_sessions_sanitize_persistent_metadata_and_event_summaries() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let session_id = "session-sanitize";
    let session_dir = data.join("sessions").join(session_id);
    std::fs::create_dir_all(&session_dir).unwrap();

    let now = "2026-11-06T15:00:00Z";
    let meta = serde_json::json!({
        "id": session_id,
        "workspace": "\u{001b}]0;owned\u{0007}C:/workspace",
        "provider": "provider\u{001b}[31m",
        "model": "model\u{001b}[0m",
        "policy": "read-only",
        "status": "completed\u{001b}[31m",
        "turns": 1,
        "usage": {
            "input_tokens": 1,
            "output_tokens": 2,
            "cached_input_tokens": 0
        },
        "cost_usd": 0.0,
        "created_at": now,
        "updated_at": now,
        "raven_version": "0.2.0"
    });
    std::fs::write(
        session_dir.join("meta.json"),
        serde_json::to_vec(&meta).unwrap()
    )
    .unwrap();

    let event = Event::new(
        session_id,
        1,
        EventKind::Notice,
        "\u{001b}[31mred summary\u{001b}[0m",
        serde_json::json!({ "message": "\u{001b}]0;hidden\u{0007}" }),
        GENESIS_HASH,
    );
    std::fs::write(
        session_dir.join("events.jsonl"),
        format!("{}\n", serde_json::to_string(&event).unwrap()),
    )
    .unwrap();

    let shown = Command::new(raven())
        .arg("show")
        .arg(session_id)
        .env("RAVEN_DATA_DIR", &data)
        .output()
        .unwrap();
    assert_success(&shown, "show sanitized session");
    let show_stdout = String::from_utf8_lossy(&shown.stdout);
    assert!(
        !show_stdout.contains('\u{1b}'),
        "show emitted a terminal escape from persistent data: {show_stdout:?}"
    );
    assert!(
        show_stdout.contains("red summary"),
        "the sanitized event summary was not retained: {show_stdout}"
    );

    let listed = Command::new(raven())
        .arg("sessions")
        .arg("--limit")
        .arg("10")
        .env("RAVEN_DATA_DIR", &data)
        .output()
        .unwrap();
    assert_success(&listed, "sessions list with terminal-control metadata");
    let list_stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(
        !list_stdout.contains('\u{1b}'),
        "sessions list emitted a terminal escape from persistent data: {list_stdout:?}"
    );
}

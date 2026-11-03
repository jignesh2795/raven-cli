//! Secret canary suite.
//!
//! Planted credentials in workspace files must never reach a provider payload,
//! the audit log, or a human-visible approval diff. `SpyProvider` records the
//! exact message list of every request it receives, which is what would be sent
//! over the wire, and the raw event log is checked the same way.
//!
//! This is a canary test, not a proof: it fails loudly if a redaction pattern
//! regresses or a new code path forgets to redact, but it cannot cover secret
//! formats the redactor has never seen.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use raven_core::agent::{Agent, AgentConfig, EventSink, NoopObserver};
use raven_core::cost::CostTracker;
use raven_core::events::EventKind;
use raven_core::policy::{PathPolicy, Policy, PolicyMode};
use raven_core::provider::{ChatRequest, Provider, ProviderEvent, ProviderStream};
use raven_core::redact::Redactor;
use raven_core::tool::{ApprovalRequest, Approver, Decision, ToolRegistry};
use raven_core::Result;
use raven_store::{read_events, Store};
use raven_tools::{read_only_registry, workspace_write_registry};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

/// Distinctive non-secret substrings. If any of these survives into a provider
/// payload, the log, or a diff, the canary fires.
const MARKERS: &[&str] = &[
    "CANARY0011223344556677889900",
    "CANARY00112233445566778899",
    "0CANARY0011223344",
    "CANARYbearer0077665544",
    "CANARYpass11335577",
    "CANARYpw5544332211",
    "MIIEowIBAAKCAQEA",
    "CANARYsig998877665544",
];

const CANARY_FILES: &[(&str, &str)] = &[
    (
        "openai.txt",
        "config line\nOPENAI_API_KEY=sk-CANARY0011223344556677889900\nend\n",
    ),
    (
        "github.txt",
        "config line\nGITHUB_TOKEN=ghp_CANARY00112233445566778899\nend\n",
    ),
    ("aws.txt", "AWS_ACCESS_KEY_ID=AKIA0CANARY0011223344\n"),
    ("bearer.txt", "Authorization: Bearer CANARYbearer0077665544\n"),
    (
        "url.txt",
        "DSN=https://user:CANARYpass11335577@db.example.com/app\n",
    ),
    ("kv.txt", "password = \"CANARYpw5544332211\"\n"),
    (
        "private_key.txt",
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0123456789abcdefghijklmnopqrstuvwxyz\n-----END RSA PRIVATE KEY-----\n",
    ),
    (
        "jwt.txt",
        "TOKEN=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJjYW5hcnkifQ.CANARYsig998877665544\n",
    ),
];

enum Step {
    Read(String),
    Edit {
        path: String,
        old: String,
        new: String,
    },
    Finish,
}

struct SpyProvider {
    /// Every outbound request, as the JSON that would hit the wire.
    requests: Arc<Mutex<Vec<String>>>,
    steps: Mutex<Vec<Step>>,
    call: AtomicUsize,
}

impl SpyProvider {
    fn new(steps: Vec<Step>) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            steps: Mutex::new(steps),
            call: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl Provider for SpyProvider {
    fn name(&self) -> &str {
        "spy"
    }

    fn model(&self) -> &str {
        "spy-model"
    }

    async fn stream_chat(
        &self,
        request: ChatRequest,
        _cancel: CancellationToken,
    ) -> Result<ProviderStream> {
        self.requests
            .lock()
            .unwrap()
            .push(serde_json::to_string(&request.messages).expect("messages serialize"));

        let index = self.call.fetch_add(1, Ordering::SeqCst);
        // The stream must own its data, so the strings are cloned out of the
        // step table before the lock guard is dropped.
        let call = {
            let steps = self.steps.lock().unwrap();
            match steps.get(index) {
                Some(Step::Read(path)) => Some((
                    "call_read".to_string(),
                    "read_file".to_string(),
                    json!({ "path": path }).to_string(),
                )),
                Some(Step::Edit { path, old, new }) => Some((
                    "call_edit".to_string(),
                    "edit_file".to_string(),
                    json!({ "path": path, "old": old, "new": new }).to_string(),
                )),
                Some(Step::Finish) | None => None,
            }
        };

        // One stream for both shapes: two `async_stream::stream!` blocks would
        // be two distinct types and could not be matched against each other.
        let stream = async_stream::stream! {
            match call {
                Some((id, name, arguments)) => {
                    yield Ok(ProviderEvent::ToolCallDelta {
                        index: 0,
                        id: Some(id),
                        name: Some(name),
                        arguments: Some(arguments),
                    });
                    yield Ok(ProviderEvent::Done { finish_reason: Some("tool_calls".into()) });
                }
                None => {
                    yield Ok(ProviderEvent::TextDelta("done".into()));
                    yield Ok(ProviderEvent::Done { finish_reason: Some("stop".into()) });
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

/// Approves every write while recording what was shown for approval.
struct SpyApprover {
    requests: Arc<Mutex<Vec<ApprovalRequest>>>,
}

#[async_trait]
impl Approver for SpyApprover {
    async fn approve(&self, request: ApprovalRequest) -> Decision {
        self.requests.lock().unwrap().push(request);
        Decision::Approved
    }
}

struct MemorySink {
    events: Mutex<Vec<(String, String)>>,
}

impl EventSink for MemorySink {
    fn append(&self, kind: EventKind, summary: &str, _data: Value) -> Result<()> {
        self.events
            .lock()
            .unwrap()
            .push((kind.as_str().to_string(), summary.to_string()));
        Ok(())
    }
}

fn agent(
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    sink: Arc<dyn EventSink>,
    workspace: &Path,
    approver: Arc<dyn Approver>,
    policy: Policy,
) -> Agent {
    Agent::new(
        "canary-session".into(),
        "system prompt".into(),
        provider,
        tools,
        sink,
        Arc::new(NoopObserver),
        policy,
        Arc::new(Redactor::default()),
        Arc::new(PathPolicy::new(workspace).unwrap()),
        workspace.to_path_buf(),
        64 * 1024,
        AgentConfig {
            max_turns: 12,
            temperature: None,
            max_tokens: None,
            provider_name: "spy".into(),
            model: "spy-model".into(),
        },
        CostTracker::new(None, None),
        CancellationToken::new(),
    )
    .with_approver(approver)
}

fn assert_clean(label: &str, payloads: &[String]) {
    for (index, payload) in payloads.iter().enumerate() {
        for marker in MARKERS {
            assert!(
                !payload.contains(marker),
                "{label} #{index} leaked the canary marker '{marker}': {payload}"
            );
        }
    }
}

fn write_canaries(workspace: &Path) {
    std::fs::create_dir_all(workspace).unwrap();
    for (name, content) in CANARY_FILES {
        std::fs::write(workspace.join(name), content).unwrap();
    }
}

fn read_steps() -> Vec<Step> {
    let mut steps: Vec<Step> = CANARY_FILES
        .iter()
        .map(|(name, _)| Step::Read((*name).to_string()))
        .collect();
    steps.push(Step::Finish);
    steps
}

fn edit_steps() -> Vec<Step> {
    vec![
        Step::Edit {
            path: "openai.txt".into(),
            old: "config line".into(),
            new: "config line (patched)".into(),
        },
        Step::Finish,
    ]
}

fn memory_sink() -> Arc<MemorySink> {
    Arc::new(MemorySink {
        events: Mutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn canaries_never_reach_a_provider_payload() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().to_path_buf();
    write_canaries(&workspace);

    let provider = Arc::new(SpyProvider::new(read_steps()));
    let recorded = provider.requests.clone();
    let mut agent = agent(
        provider,
        Arc::new(read_only_registry()),
        memory_sink(),
        &workspace,
        Arc::new(raven_core::tool::DenyAll),
        Policy::read_only(),
    );
    agent.run("Read every canary file".into()).await.unwrap();

    let payloads = recorded.lock().unwrap();
    assert!(
        payloads.len() > CANARY_FILES.len(),
        "the fixture did not drive {} tool calls",
        CANARY_FILES.len()
    );
    assert_clean("provider payload", &payloads);
}

#[tokio::test]
async fn canaries_never_reach_the_audit_log() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().to_path_buf();
    write_canaries(&workspace);

    let store = Arc::new(
        Store::create(
            temp.path(),
            &workspace,
            "spy",
            "spy-model",
            PolicyMode::ReadOnly.label(),
        )
        .unwrap(),
    );
    let mut agent = agent(
        Arc::new(SpyProvider::new(read_steps())),
        Arc::new(read_only_registry()),
        store.clone(),
        &workspace,
        Arc::new(raven_core::tool::DenyAll),
        Policy::read_only(),
    );
    agent.run("Read every canary file".into()).await.unwrap();
    store
        .finish("completed", agent.turns(), agent.usage(), agent.cost_usd())
        .unwrap();

    let events = read_events(temp.path(), store.session_id()).unwrap();
    assert_eq!(
        events.last().unwrap().kind,
        EventKind::SessionFinished,
        "the session did not close cleanly"
    );
    assert_clean("audit log", &[serde_json::to_string(&events).unwrap()]);
}

#[tokio::test]
async fn canaries_never_reach_an_approval_diff() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().to_path_buf();
    write_canaries(&workspace);

    // The edit sits directly above the OpenAI canary line, so the unified diff
    // context contains the secret: this is the furthest a canary can travel.
    let provider = Arc::new(SpyProvider::new(edit_steps()));
    let recorded = provider.requests.clone();
    let shown = Arc::new(Mutex::new(Vec::new()));
    let approver = Arc::new(SpyApprover {
        requests: shown.clone(),
    });

    let store = Arc::new(
        Store::create(
            temp.path(),
            &workspace,
            "spy",
            "spy-model",
            PolicyMode::WorkspaceWrite.label(),
        )
        .unwrap(),
    );
    let mut agent = agent(
        provider,
        Arc::new(workspace_write_registry()),
        store.clone(),
        &workspace,
        approver,
        Policy::workspace_write(),
    );
    agent.run("Patch the config line".into()).await.unwrap();
    store
        .finish("completed", agent.turns(), agent.usage(), agent.cost_usd())
        .unwrap();

    let shown = shown.lock().unwrap();
    assert_eq!(shown.len(), 1, "the edit was never presented for approval");

    let on_disk = std::fs::read_to_string(workspace.join("openai.txt")).unwrap();
    assert!(
        on_disk.contains("config line (patched)"),
        "the edit was not applied: {on_disk}"
    );
    // Non-vacuity: the secret is still present in the file, so it only reached
    // the diff window rather than disappearing on its own.
    assert!(
        on_disk.contains("CANARY0011223344556677889900"),
        "the canary file was unexpectedly rewritten"
    );

    let rendered: Vec<String> = shown
        .iter()
        .map(|request| format!("{}\n{}\n{}", request.summary, request.path, request.diff))
        .collect();
    assert_clean("approval diff", &rendered);
    assert_clean("provider payload", &recorded.lock().unwrap());
    assert_clean(
        "audit log",
        &[serde_json::to_string(&read_events(temp.path(), store.session_id()).unwrap()).unwrap()],
    );
}

#[tokio::test]
async fn a_denied_edit_leaves_the_file_untouched_without_echoing_it() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().to_path_buf();
    write_canaries(&workspace);

    let provider = Arc::new(SpyProvider::new(edit_steps()));
    let recorded = provider.requests.clone();
    let mut agent = agent(
        provider,
        Arc::new(workspace_write_registry()),
        memory_sink(),
        &workspace,
        Arc::new(raven_core::tool::DenyAll),
        Policy::workspace_write(),
    );
    let summary = agent.run("Patch the config line".into()).await.unwrap();
    assert!(summary.turns > 1, "the fixture did not run to completion");

    let on_disk = std::fs::read_to_string(workspace.join("openai.txt")).unwrap();
    assert!(
        on_disk.contains("CANARY0011223344556677889900") && !on_disk.contains("patched"),
        "a denied edit was applied: {on_disk}"
    );

    // The denial is reported back to the model as a tool error. That error text
    // must not echo the file contents over the wire.
    assert_clean("error text", &recorded.lock().unwrap());
}

#[tokio::test]
async fn every_planted_canary_is_one_the_redactor_recognizes() {
    // Guard against a vacuous suite: if the redactor ignored a planted canary,
    // the assertions above would pass no matter what the tools did.
    let redactor = Redactor::default();
    for (name, content) in CANARY_FILES {
        let redacted = redactor.redact(content);
        assert_ne!(
            redacted, *content,
            "{name}: the redactor ignored this canary, so the suite proves nothing"
        );
        for marker in MARKERS {
            assert!(
                !redacted.contains(marker),
                "{name}: the redactor left the canary visible:\n{redacted}"
            );
        }
    }
}

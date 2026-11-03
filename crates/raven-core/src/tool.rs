use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::message::ToolSpec;
use crate::policy::PathPolicy;
use crate::redact::Redactor;
use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    ReadOnly,
    Write,
    Execute,
    Network,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Write => "write",
            Self::Execute => "execute",
            Self::Network => "network",
        }
    }
}

/// A file change shown to the human before it is applied. Everything in here is
/// already redacted and control-stripped by the tool that built it.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub tool: String,
    pub path: String,
    pub summary: String,
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Approved,
    Denied(String),
}

/// Decides whether a proposed change may be written. Implementations run on the
/// agent task, so a blocking terminal prompt is acceptable but should be brief.
#[async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, request: ApprovalRequest) -> Decision;
}

/// Fail-closed default: any change that reaches an agent without an explicit
/// approver is denied rather than written silently.
#[derive(Debug, Default)]
pub struct DenyAll;

#[async_trait]
impl Approver for DenyAll {
    async fn approve(&self, _request: ApprovalRequest) -> Decision {
        Decision::Denied("write approval is not configured".into())
    }
}

/// Approves every change. Used by tests and by `--yes`.
#[derive(Debug, Default)]
pub struct ApproveAll;

#[async_trait]
impl Approver for ApproveAll {
    async fn approve(&self, _request: ApprovalRequest) -> Decision {
        Decision::Approved
    }
}

#[derive(Clone)]
pub struct ToolContext {
    pub workspace: PathBuf,
    pub path_policy: Arc<PathPolicy>,
    pub redactor: Arc<Redactor>,
    pub output_cap: usize,
    pub approver: Arc<dyn Approver>,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub truncated: bool,
    /// Structured details for the audit log, e.g. hashes for an applied edit.
    pub data: Option<Value>,
}

impl ToolOutput {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            truncated: false,
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn kind(&self) -> ToolKind;
    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput>;
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.spec().name;
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

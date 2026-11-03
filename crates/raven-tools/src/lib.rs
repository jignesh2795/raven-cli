#![deny(unsafe_code)]

//! Tools for the agent loop.
//!
//! M0 exposes read-only tools. M1 adds exact edits and file creation behind the
//! workspace-write policy and the human approval gate. Command execution and
//! network tools arrive in later milestones.

mod create_file;
mod diff;
mod edit_file;
mod list_dir;
mod read_file;
mod search;
mod write_support;

pub use create_file::CreateFileTool;
pub use edit_file::EditFileTool;
pub use list_dir::ListDirTool;
pub use read_file::ReadFileTool;
pub use search::SearchTool;

use std::sync::Arc;

use raven_core::tool::ToolRegistry;

/// Read-only tool set: no writes, no command execution, no network.
pub fn read_only_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadFileTool));
    registry.register(Arc::new(ListDirTool));
    registry.register(Arc::new(SearchTool));
    registry
}

/// Read-only tools plus exact edits and file creation. The policy must also be
/// `workspace-write` and every write is presented to the approver.
pub fn workspace_write_registry() -> ToolRegistry {
    let mut registry = read_only_registry();
    registry.register(Arc::new(EditFileTool));
    registry.register(Arc::new(CreateFileTool));
    registry
}

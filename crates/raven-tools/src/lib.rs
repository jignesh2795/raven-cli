#![deny(unsafe_code)]

//! Read-only tools for M0. Write and execution tools arrive in later
//! milestones behind the policy and sandbox crates.

mod list_dir;
mod read_file;
mod search;

pub use list_dir::ListDirTool;
pub use read_file::ReadFileTool;
pub use search::SearchTool;

use std::sync::Arc;

use raven_core::tool::ToolRegistry;

/// The default M0 tool set: no writes, no command execution, no network.
pub fn read_only_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadFileTool));
    registry.register(Arc::new(ListDirTool));
    registry.register(Arc::new(SearchTool));
    registry
}

use async_trait::async_trait;
use ignore::WalkBuilder;
use raven_core::message::ToolSpec;
use raven_core::policy::is_protected_path;
use raven_core::redact::{sanitize_text, truncate_utf8};
use raven_core::tool::{Tool, ToolContext, ToolKind, ToolOutput};
use raven_core::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_ENTRIES: usize = 1000;

#[derive(Debug, Default)]
pub struct ListDirTool;

#[derive(Debug, Deserialize)]
struct Args {
    #[serde(default = "default_path")]
    path: String,
    #[serde(default = "default_depth")]
    depth: usize,
}

fn default_path() -> String {
    ".".into()
}

fn default_depth() -> usize {
    1
}

#[async_trait]
impl Tool for ListDirTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_dir".into(),
            description: "List files and folders inside the workspace. Respects .gitignore and hides protected paths.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative directory. Defaults to the workspace root."
                    },
                    "depth": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 3,
                        "description": "Recursion depth. Defaults to 1."
                    }
                },
                "additionalProperties": false
            }),
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ReadOnly
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let args: Args = serde_json::from_value(args)
            .map_err(|error| Error::Tool(format!("invalid list_dir arguments: {error}")))?;
        let root = ctx.path_policy.resolve_existing(&args.path)?;
        if !root.is_dir() {
            return Err(Error::Tool(format!("'{}' is not a directory", args.path)));
        }

        let depth = args.depth.clamp(1, 3);
        let mut entries = Vec::new();
        let mut truncated = false;

        let walker = WalkBuilder::new(&root)
            .max_depth(Some(depth))
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .filter_entry(|entry| {
                entry.file_name().to_string_lossy() != ".git"
                    && !is_protected_path(entry.path())
            })
            .build();

        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if entry.path() == root {
                continue;
            }
            if entries.len() >= MAX_ENTRIES {
                truncated = true;
                break;
            }

            let relative = ctx.path_policy.display_relative(entry.path());
            if relative.is_empty() {
                continue;
            }
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                entries.push(format!("{relative}/"));
            } else {
                let size = entry
                    .metadata()
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                entries.push(format!("{relative} ({size} bytes)"));
            }
        }

        entries.sort();
        let mut output = if entries.is_empty() {
            format!(
                "Directory: {} (empty)\n",
                ctx.path_policy.display_relative(&root)
            )
        } else {
            format!(
                "Directory: {}\n{}\n",
                ctx.path_policy.display_relative(&root),
                entries.join("\n")
            )
        };

        if truncated {
            output.push_str(&format!(
                "\n[listing truncated at {MAX_ENTRIES} entries]\n"
            ));
        }

        let output = sanitize_text(&output);
        let (content, capped) = truncate_utf8(&output, ctx.output_cap);
        Ok(ToolOutput {
            content: content.to_string(),
            truncated: truncated || capped,
        })
    }
}

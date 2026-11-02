use std::fs::File;
use std::io::Read;

use async_trait::async_trait;
use raven_core::message::ToolSpec;
use raven_core::redact::{sanitize_text, truncate_utf8};
use raven_core::tool::{Tool, ToolContext, ToolKind, ToolOutput};
use raven_core::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_READ_BYTES: usize = 4 * 1024 * 1024;
const MAX_LIMIT: usize = 5000;
const DEFAULT_LIMIT: usize = 2000;

#[derive(Debug, Default)]
pub struct ReadFileTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a UTF-8 text file inside the workspace. Returns numbered lines. Protected paths such as .env and .git are denied.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path to the file."
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "First line to return (1-based). Defaults to 1."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_LIMIT,
                        "description": "Maximum number of lines. Defaults to 2000."
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ReadOnly
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let args: Args = serde_json::from_value(args)
            .map_err(|error| Error::Tool(format!("invalid read_file arguments: {error}")))?;
        let path = ctx.path_policy.resolve_existing(&args.path)?;

        if path.is_dir() {
            return Err(Error::Tool(format!(
                "'{}' is a directory; use list_dir",
                args.path
            )));
        }

        let metadata = std::fs::metadata(&path)?;
        if metadata.len() == 0 {
            return Ok(ToolOutput::new(format!(
                "File: {} (empty)",
                ctx.path_policy.display_relative(&path)
            )));
        }

        let mut buffer = Vec::new();
        File::open(&path)?
            .take(MAX_READ_BYTES as u64)
            .read_to_end(&mut buffer)?;
        let file_truncated = metadata.len() > buffer.len() as u64;

        if buffer.iter().take(8192).any(|byte| *byte == 0) {
            return Err(Error::Tool(format!(
                "'{}' looks like a binary file",
                args.path
            )));
        }

        let text = String::from_utf8_lossy(&buffer);
        let text = sanitize_text(&text);
        let lines: Vec<&str> = text.lines().collect();

        let start = args.offset.unwrap_or(1).saturating_sub(1);
        if start >= lines.len() {
            return Ok(ToolOutput::new(format!(
                "File: {} has {} lines; offset {} is past the end",
                ctx.path_policy.display_relative(&path),
                lines.len(),
                start + 1
            )));
        }

        let limit = args
            .limit
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, MAX_LIMIT);
        let end = (start + limit).min(lines.len());

        let mut output = format!(
            "File: {} (lines {}-{} of {})\n",
            ctx.path_policy.display_relative(&path),
            start + 1,
            end,
            lines.len()
        );

        let width = end.to_string().len();
        for (index, line) in lines[start..end].iter().enumerate() {
            let number = start + index + 1;
            let redacted = ctx.redactor.redact(line);
            output.push_str(&format!("{number:>width$}  {redacted}\n"));
        }

        let notes = if file_truncated {
            "\n[file truncated at 4 MiB]\n"
        } else if end < lines.len() {
            "\n[use offset/limit to read the remaining lines]\n"
        } else {
            ""
        };
        if !notes.is_empty() {
            output.push_str(notes);
        }

        let output = sanitize_text(&output);
        let (content, truncated) = truncate_utf8(&output, ctx.output_cap);
        Ok(ToolOutput {
            content: content.to_string(),
            truncated: truncated || file_truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raven_core::policy::PathPolicy;
    use raven_core::redact::Redactor;
    use std::sync::Arc;

    fn context(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace: root.to_path_buf(),
            path_policy: Arc::new(PathPolicy::new(root).unwrap()),
            redactor: Arc::new(Redactor::default()),
            output_cap: 64 * 1024,
        }
    }

    #[tokio::test]
    async fn reads_numbered_lines_and_redacts_secrets() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("test.txt"),
            "alpha\nOPENAI_API_KEY=sk-1234567890abcdefghijklmnop\nomega\n",
        )
        .unwrap();

        let tool = ReadFileTool;
        let output = tool
            .call(json!({ "path": "test.txt", "offset": 1, "limit": 2 }), &context(temp.path()))
            .await
            .unwrap();

        assert!(output.content.contains("1  alpha"));
        assert!(output.content.contains("2  OPENAI_API_KEY=[REDACTED]"));
        assert!(!output.content.contains("sk-1234567890"));
        assert!(output.content.contains("3  omega") == false);
    }
}

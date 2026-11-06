use async_trait::async_trait;
use raven_core::message::ToolSpec;
use raven_core::redact::{sanitize_text, truncate_utf8};
use raven_core::tool::{ApprovalRequest, Decision, Tool, ToolContext, ToolKind, ToolOutput};
use raven_core::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::diff;
use crate::write_support::{self, MAX_FILE_BYTES};

const MAX_DIFF_BYTES: usize = 16 * 1024;

/// Create a new text file. Fails if the path already exists, so it can never be
/// used to clobber a file the model did not read.
#[derive(Debug, Default)]
pub struct CreateFileTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for CreateFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "create_file".into(),
            description: "Create a new UTF-8 text file inside the workspace. Fails if the path already exists; use edit_file to change an existing file. The new file is shown to the human as a diff and requires approval. Writes are atomic and verified by SHA-256.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path for the new file."
                    },
                    "content": {
                        "type": "string",
                        "description": "Complete file content."
                    }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Write
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let args: Args = serde_json::from_value(args)
            .map_err(|error| Error::Tool(format!("invalid create_file arguments: {error}")))?;

        if args.content.len() > MAX_FILE_BYTES {
            return Err(Error::Tool("content exceeds the 4 MiB edit limit".into()));
        }

        let path = ctx.path_policy.resolve_for_write(&args.path)?;
        let relative = ctx.path_policy.display_relative(&path);

        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(Error::Tool(format!(
                "'{relative}' already exists; use edit_file to change it"
            )));
        }

        let bytes = args.content.as_bytes();
        let after_sha = write_support::sha256_hex(bytes);
        let diff = diff::unified(&relative, "", &args.content);
        let added = args.content.lines().count();

        let summary = format!("{relative}: create (+{added})");
        let request = ApprovalRequest {
            tool: "create_file".into(),
            path: relative.clone(),
            summary: summary.clone(),
            diff: ctx.redactor.redact(&sanitize_text(&diff.text)),
        };
        match ctx.approver.approve(request).await {
            Decision::Approved => {}
            Decision::Denied(reason) => {
                return Err(Error::Tool(format!("create was not applied: {reason}")));
            }
        }

        // The path must still be free: another process (or a queued tool call)
        // may have created it while the human was reviewing the diff.
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(Error::Tool(format!(
                "'{relative}' appeared while the create was being approved; not overwriting it"
            )));
        }

        write_support::atomic_create(&path, bytes)?;

        let after = std::fs::read(&path)?;
        if write_support::sha256_hex(&after) != after_sha {
            return Err(Error::Tool(format!(
                "'{relative}' did not match the expected content after the write; inspect the file before continuing"
            )));
        }

        let audit_diff = {
            let redacted = ctx.redactor.redact(&sanitize_text(&diff.text));
            truncate_utf8(&redacted, MAX_DIFF_BYTES).0.to_string()
        };

        let data = json!({
            "kind": "edit",
            "op": "create",
            "path": relative.clone(),
            "before_sha256": Value::Null,
            "after_sha256": after_sha,
            "added": added,
            "removed": 0,
            "replacements": Value::Null,
            "approval": "approved",
            "diff": audit_diff,
            "diff_truncated": diff.truncated,
        });

        Ok(ToolOutput::new(format!(
            "Created {relative} ({added} lines).\nsha256 {}\nRe-read the file before making further edits.",
            &after_sha[..16]
        ))
        .with_data(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raven_core::policy::PathPolicy;
    use raven_core::redact::Redactor;
    use raven_core::tool::{ApprovalRequest, ApproveAll, Approver, Decision, DenyAll};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct CreateTargetDuringApproval {
        target: PathBuf,
        contents: &'static str,
    }

    #[async_trait::async_trait]
    impl Approver for CreateTargetDuringApproval {
        async fn approve(&self, _request: ApprovalRequest) -> Decision {
            std::fs::write(&self.target, self.contents).unwrap();
            Decision::Approved
        }
    }

    fn context(root: &std::path::Path, approver: Arc<dyn Approver>) -> ToolContext {
        ToolContext {
            workspace: root.to_path_buf(),
            path_policy: Arc::new(PathPolicy::new(root).unwrap()),
            redactor: Arc::new(Redactor::default()),
            output_cap: 64 * 1024,
            approver,
        }
    }

    #[tokio::test]
    async fn creates_a_new_file_and_records_the_hash() {
        let temp = tempfile::tempdir().unwrap();
        let output = CreateFileTool
            .call(
                json!({ "path": "notes/today.md", "content": "# Today\n" }),
                &context(temp.path(), Arc::new(ApproveAll)),
            )
            .await;

        // The parent directory must already exist.
        assert!(output.is_err());

        std::fs::create_dir(temp.path().join("notes")).unwrap();
        let output = CreateFileTool
            .call(
                json!({ "path": "notes/today.md", "content": "# Today\n" }),
                &context(temp.path(), Arc::new(ApproveAll)),
            )
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(temp.path().join("notes/today.md")).unwrap(),
            "# Today\n"
        );
        let data = output.data.unwrap();
        assert_eq!(data["op"], "create");
        assert!(data["before_sha256"].is_null());
        assert_eq!(data["added"], 1);
    }

    #[tokio::test]
    async fn refuses_to_overwrite_an_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("exists.txt"), "keep\n").unwrap();

        let error = CreateFileTool
            .call(
                json!({ "path": "exists.txt", "content": "replace\n" }),
                &context(temp.path(), Arc::new(ApproveAll)),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("already exists"));
        assert_eq!(
            std::fs::read_to_string(temp.path().join("exists.txt")).unwrap(),
            "keep\n"
        );
    }

    #[tokio::test]
    async fn a_denied_create_writes_nothing() {
        let temp = tempfile::tempdir().unwrap();

        let error = CreateFileTool
            .call(
                json!({ "path": "new.txt", "content": "hello\n" }),
                &context(temp.path(), Arc::new(DenyAll)),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("not applied"));
        assert!(!temp.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn a_target_appearing_during_approval_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("raced.txt");
        let approver = Arc::new(CreateTargetDuringApproval {
            target: target.clone(),
            contents: "created during approval\n",
        });

        let error = CreateFileTool
            .call(
                json!({ "path": "raced.txt", "content": "model contents\n" }),
                &context(temp.path(), approver),
            )
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("appeared while the create was being approved"),
            "unexpected error: {error}"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "created during approval\n"
        );
        let leftovers: Vec<String> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(".raven-tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files were left behind");
    }
}

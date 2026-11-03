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

/// Exact `old`/`new` search-replace. Never line-number based, never a whole-file
/// rewrite: the old text must be found verbatim and (by default) exactly once.
#[derive(Debug, Default)]
pub struct EditFileTool;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    old: String,
    new: String,
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for EditFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".into(),
            description: "Replace exact text in an existing workspace file. 'old' must appear exactly once unless replace_all is true; copy it from read_file without the line-number prefix. The file's UTF-8 BOM and line-ending style are preserved. The change is shown to the human as a diff and requires approval. Writes are atomic and verified by SHA-256.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative path to an existing file."
                    },
                    "old": {
                        "type": "string",
                        "description": "Exact text to replace, including its indentation."
                    },
                    "new": {
                        "type": "string",
                        "description": "Replacement text. Use an empty string to delete."
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace every occurrence instead of requiring a unique match. Defaults to false."
                    }
                },
                "required": ["path", "old", "new"],
                "additionalProperties": false
            }),
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Write
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let args: Args = serde_json::from_value(args)
            .map_err(|error| Error::Tool(format!("invalid edit_file arguments: {error}")))?;

        if args.old.is_empty() {
            return Err(Error::Tool("old text is empty".into()));
        }
        if args.old.len() > MAX_FILE_BYTES || args.new.len() > MAX_FILE_BYTES {
            return Err(Error::Tool(
                "old or new text exceeds the 4 MiB edit limit".into(),
            ));
        }
        if args.old == args.new {
            return Err(Error::Tool(
                "old and new text are identical; nothing to change".into(),
            ));
        }

        let path = ctx.path_policy.resolve_for_write(&args.path)?;
        let relative = ctx.path_policy.display_relative(&path);
        let file = write_support::read_text_file(&path)?;

        let eol = write_support::detect_eol(&file.text);
        let original_lf = write_support::to_lf(&file.text);
        if write_support::to_eol(&original_lf, eol) != file.text {
            return Err(Error::Tool(format!(
                "'{relative}' has mixed or unusual line endings; normalize it before editing"
            )));
        }

        let old = write_support::to_lf(&args.old);
        let new = write_support::to_lf(&args.new);
        let occurrences: Vec<usize> = original_lf
            .match_indices(&old)
            .map(|(offset, _)| offset)
            .collect();

        if occurrences.is_empty() {
            return Err(Error::Tool(format!(
                "old text was not found in '{relative}'; copy the exact text from read_file, without the line-number prefix"
            )));
        }
        if occurrences.len() > 1 && !args.replace_all {
            let lines: Vec<String> = occurrences
                .iter()
                .map(|offset| write_support::line_number(&original_lf, *offset).to_string())
                .collect();
            return Err(Error::Tool(format!(
                "old text appears {} times in '{relative}' (lines {}); include more surrounding context or set replace_all=true",
                occurrences.len(),
                lines.join(", ")
            )));
        }

        let updated_text = write_support::to_eol(&original_lf.replace(&old, &new), eol);
        if updated_text == file.text {
            return Err(Error::Tool("the edit would not change the file".into()));
        }

        let original_bytes = write_support::encode(&file.text, file.bom);
        let updated_bytes = write_support::encode(&updated_text, file.bom);
        let before_sha = write_support::sha256_hex(&original_bytes);
        let after_sha = write_support::sha256_hex(&updated_bytes);

        let diff = diff::unified(&relative, &file.text, &updated_text);
        let replacements = occurrences.len();
        let summary = format!(
            "{relative}: {replacements} replacement{}, +{} -{}",
            if replacements == 1 { "" } else { "s" },
            diff.added,
            diff.removed
        );

        let request = ApprovalRequest {
            tool: "edit_file".into(),
            path: relative.clone(),
            summary: summary.clone(),
            diff: ctx.redactor.redact(&sanitize_text(&diff.text)),
        };
        match ctx.approver.approve(request).await {
            Decision::Approved => {}
            Decision::Denied(reason) => {
                return Err(Error::Tool(format!("edit was not applied: {reason}")));
            }
        }

        // Re-check the pre-image right before the swap. This narrows, but does
        // not eliminate, the window for another process to change the file while
        // the human is reviewing the diff.
        let current = std::fs::read(&path)?;
        if write_support::sha256_hex(&current) != before_sha {
            return Err(Error::Tool(format!(
                "'{relative}' changed while the edit was being approved; read it again and retry"
            )));
        }

        write_support::atomic_write(&path, &updated_bytes)?;

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
            "op": "edit",
            "path": relative.clone(),
            "before_sha256": before_sha,
            "after_sha256": after_sha,
            "added": diff.added,
            "removed": diff.removed,
            "replacements": replacements,
            "approval": "approved",
            "diff": audit_diff,
            "diff_truncated": diff.truncated,
        });

        let content = format!(
            "Edited {relative}: {replacements} replacement{} (+{} -{}).\nsha256 {}\nRe-read the file before making further edits.",
            if replacements == 1 { "" } else { "s" },
            diff.added,
            diff.removed,
            &after_sha[..16]
        );

        Ok(ToolOutput::new(content).with_data(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raven_core::policy::PathPolicy;
    use raven_core::redact::Redactor;
    use raven_core::tool::{ApproveAll, Approver, DenyAll};
    use std::sync::Arc;

    fn context(root: &std::path::Path, approver: Arc<dyn Approver>) -> ToolContext {
        ToolContext {
            workspace: root.to_path_buf(),
            path_policy: Arc::new(PathPolicy::new(root).unwrap()),
            redactor: Arc::new(Redactor::default()),
            output_cap: 64 * 1024,
            approver,
        }
    }

    async fn edit(
        root: &std::path::Path,
        args: Value,
        approver: Arc<dyn Approver>,
    ) -> Result<ToolOutput> {
        EditFileTool.call(args, &context(root, approver)).await
    }

    #[tokio::test]
    async fn replaces_a_unique_string_and_records_hashes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("main.rs"),
            "fn main() {\n    let x = 1;\n}\n",
        )
        .unwrap();

        let output = edit(
            temp.path(),
            json!({
                "path": "main.rs",
                "old": "    let x = 1;",
                "new": "    let x = 2;"
            }),
            Arc::new(ApproveAll),
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(temp.path().join("main.rs")).unwrap(),
            "fn main() {\n    let x = 2;\n}\n"
        );
        let data = output.data.unwrap();
        assert_eq!(data["kind"], "edit");
        assert_eq!(data["added"], 1);
        assert_eq!(data["removed"], 1);
        assert_eq!(data["replacements"], 1);
        assert_eq!(data["before_sha256"].as_str().unwrap().len(), 64);
        assert_ne!(data["before_sha256"], data["after_sha256"]);
    }

    #[tokio::test]
    async fn refuses_an_ambiguous_match_without_replace_all() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("dup.txt"), "hello hello\n").unwrap();

        let error = edit(
            temp.path(),
            json!({ "path": "dup.txt", "old": "hello", "new": "bye" }),
            Arc::new(ApproveAll),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("appears 2 times"));
        assert_eq!(
            std::fs::read_to_string(temp.path().join("dup.txt")).unwrap(),
            "hello hello\n"
        );
    }

    #[tokio::test]
    async fn replace_all_updates_every_occurrence() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("dup.txt"), "hello hello\n").unwrap();

        edit(
            temp.path(),
            json!({
                "path": "dup.txt",
                "old": "hello",
                "new": "bye",
                "replace_all": true
            }),
            Arc::new(ApproveAll),
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(temp.path().join("dup.txt")).unwrap(),
            "bye bye\n"
        );
    }

    #[tokio::test]
    async fn a_denied_edit_leaves_the_file_untouched() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("keep.txt"), "original\n").unwrap();

        let error = edit(
            temp.path(),
            json!({ "path": "keep.txt", "old": "original", "new": "changed" }),
            Arc::new(DenyAll),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("not applied"));
        assert_eq!(
            std::fs::read_to_string(temp.path().join("keep.txt")).unwrap(),
            "original\n"
        );
    }

    #[tokio::test]
    async fn preserves_crlf_line_endings_and_bom() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("windows.txt");
        std::fs::write(&path, b"\xEF\xBB\xBFone\r\ntwo\r\nthree\r\n").unwrap();

        edit(
            temp.path(),
            json!({ "path": "windows.txt", "old": "two\n", "new": "TWO\n" }),
            Arc::new(ApproveAll),
        )
        .await
        .unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, b"\xEF\xBB\xBFone\r\nTWO\r\nthree\r\n");
    }

    #[tokio::test]
    async fn rejects_nested_protected_paths() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        std::fs::write(temp.path().join(".git").join("config"), "x\n").unwrap();

        let error = edit(
            temp.path(),
            json!({ "path": ".git/config", "old": "x", "new": "y" }),
            Arc::new(ApproveAll),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("protected"));
    }
}

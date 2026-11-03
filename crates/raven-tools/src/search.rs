use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use raven_core::message::ToolSpec;
use raven_core::policy::is_protected_path;
use raven_core::redact::{sanitize_text, truncate_utf8};
use raven_core::tool::{Tool, ToolContext, ToolKind, ToolOutput};
use raven_core::{Error, Result};
use regex::RegexBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_RESULTS: usize = 500;
const DEFAULT_RESULTS: usize = 100;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 400;

#[derive(Debug, Default)]
pub struct SearchTool;

#[derive(Debug, Deserialize)]
struct Args {
    pattern: String,
    #[serde(default = "default_path")]
    path: String,
    glob: Option<String>,
    #[serde(default)]
    case_sensitive: bool,
    max_results: Option<usize>,
}

fn default_path() -> String {
    ".".into()
}

#[async_trait]
impl Tool for SearchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_files".into(),
            description: "Search workspace text files with a regular expression. Returns path:line: text. Respects .gitignore and skips protected paths.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Rust-compatible regular expression."
                    },
                    "path": {
                        "type": "string",
                        "description": "Workspace-relative file or directory. Defaults to the workspace root."
                    },
                    "glob": {
                        "type": "string",
                        "description": "Optional file glob such as '*.rs' or 'src/**/*.ts'."
                    },
                    "case_sensitive": {
                        "type": "boolean",
                        "description": "Defaults to false."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_RESULTS,
                        "description": "Defaults to 100."
                    }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
        }
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ReadOnly
    }

    async fn call(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let args: Args = serde_json::from_value(args)
            .map_err(|error| Error::Tool(format!("invalid search_files arguments: {error}")))?;
        if args.pattern.trim().is_empty() {
            return Err(Error::Tool("search pattern is empty".into()));
        }

        let regex = RegexBuilder::new(&args.pattern)
            .case_insensitive(!args.case_sensitive)
            .build()
            .map_err(|error| Error::Tool(format!("invalid regular expression: {error}")))?;

        let glob = match &args.glob {
            Some(pattern) => Some(
                Glob::new(pattern)
                    .map_err(|error| Error::Tool(format!("invalid glob: {error}")))?
                    .compile_matcher(),
            ),
            None => None,
        };

        let root = ctx.path_policy.resolve_existing(&args.path)?;
        let max_results = args
            .max_results
            .unwrap_or(DEFAULT_RESULTS)
            .clamp(1, MAX_RESULTS);

        let mut results = Vec::new();
        let mut truncated = false;

        let walker = WalkBuilder::new(&root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .max_filesize(Some(MAX_FILE_BYTES))
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                !matches!(
                    name.as_ref(),
                    ".git" | "node_modules" | "target" | ".venv" | "venv"
                ) && !is_protected_path(entry.path())
            })
            .build();

        'outer: for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Some(glob) = &glob {
                if !matches_glob(glob, entry.path(), &root) {
                    continue;
                }
            }

            let bytes = match std::fs::read(entry.path()) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            if bytes.iter().take(8192).any(|byte| *byte == 0) {
                continue;
            }

            let text = String::from_utf8_lossy(&bytes);
            let text = sanitize_text(&text);
            let relative = ctx.path_policy.display_relative(entry.path());

            for (line_index, line) in text.lines().enumerate() {
                if regex.is_match(line) {
                    let (line, _) = truncate_utf8(line, MAX_LINE_CHARS);
                    let redacted = ctx.redactor.redact(line);
                    results.push(format!("{relative}:{}: {redacted}", line_index + 1));
                    if results.len() >= max_results {
                        truncated = true;
                        break 'outer;
                    }
                }
            }
        }

        let mut output = if results.is_empty() {
            format!(
                "No matches for '{}' in {}\n",
                args.pattern,
                ctx.path_policy.display_relative(&root)
            )
        } else {
            format!(
                "Matches for '{}' ({}):\n{}\n",
                args.pattern,
                results.len(),
                results.join("\n")
            )
        };

        if truncated {
            output.push_str(&format!("\n[results truncated at {max_results} matches]\n"));
        }

        let output = sanitize_text(&output);
        let (content, capped) = truncate_utf8(&output, ctx.output_cap);
        Ok(ToolOutput {
            content: content.to_string(),
            truncated: truncated || capped,
        })
    }
}

fn matches_glob(glob: &GlobMatcher, path: &std::path::Path, root: &std::path::Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if glob.is_match(relative) {
        return true;
    }
    path.file_name()
        .map(|name| glob.is_match(std::path::Path::new(name)))
        .unwrap_or(false)
}

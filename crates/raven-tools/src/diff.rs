//! A small wrapper over `similar` that produces a bounded unified diff for the
//! approval prompt and the audit log.

use similar::{ChangeTag, TextDiff};

const MAX_DIFF_LINES: usize = 160;
const MAX_DIFF_INPUT_LINES: usize = 20_000;

pub struct Diff {
    pub text: String,
    pub added: usize,
    pub removed: usize,
    pub truncated: bool,
}

pub fn unified(path: &str, old: &str, new: &str) -> Diff {
    let old_lines = old.lines().count();
    let new_lines = new.lines().count();

    if old_lines.saturating_add(new_lines) > MAX_DIFF_INPUT_LINES {
        return Diff {
            text: format!(
                "[diff omitted: {old_lines} -> {new_lines} lines; review the summary and hashes]\n"
            ),
            added: new_lines,
            removed: old_lines,
            truncated: true,
        };
    }

    let diff = TextDiff::from_lines(old, new);
    let mut added = 0;
    let mut removed = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }

    let rendered = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string();

    let mut text = String::new();
    let mut truncated = false;
    for (index, line) in rendered.lines().enumerate() {
        if index >= MAX_DIFF_LINES {
            truncated = true;
            break;
        }
        text.push_str(line);
        text.push('\n');
    }
    if truncated {
        text.push_str("… diff truncated for review\n");
    }

    Diff {
        text,
        added,
        removed,
        truncated,
    }
}

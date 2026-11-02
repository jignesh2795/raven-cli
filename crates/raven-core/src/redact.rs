use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// Best-effort secret redaction applied before content reaches a model, the
/// event log, or the terminal. This is a mitigation, not a proof: new secret
/// formats need new patterns, and entropy-based detection is intentionally
/// left for M1.
#[derive(Debug, Clone)]
pub struct Redactor {
    enabled: bool,
}

struct Pattern {
    regex: Regex,
    replacement: &'static str,
}

static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    let mut patterns = Vec::new();
    let mut add = |pattern: &str, replacement: &'static str| {
        patterns.push(Pattern {
            regex: Regex::new(pattern).expect("built-in redaction regex"),
            replacement,
        });
    };

    // key = value / key: value forms.
    add(
        r#"(?i)(api[_-]?key|access[_-]?token|secret|password|passwd|pwd)(\s*[:=]\s*["']?)([A-Za-z0-9_\-./+=]{8,})"#,
        "${1}${2}[REDACTED]",
    );
    add(r"sk-[A-Za-z0-9_-]{20,}", "sk-[REDACTED]");
    add(r"gh[pousr]_[A-Za-z0-9]{20,}", "gh*_[REDACTED]");
    add(r"AKIA[0-9A-Z]{16}", "AKIA[REDACTED]");
    add(r"xox[baprs]-[A-Za-z0-9-]{10,}", "xox-[REDACTED]");
    add(
        r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
        "[REDACTED JWT]",
    );
    add(
        r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
        "[REDACTED PRIVATE KEY]",
    );
    add(r"(?i)bearer\s+[A-Za-z0-9\-._~+/]+=*", "Bearer [REDACTED]");
    add(
        r"(?i)(https?://[^\s:/]+:)[^@\s]+@",
        "${1}[REDACTED]@",
    );

    patterns
});

static ANSI_CSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("ansi csi regex"));
static ANSI_OSC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").expect("ansi osc regex")
});
static ANSI_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b[@-_]").expect("ansi single regex"));
static C1_CSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\u{9b}[0-?]*[ -/]*[@-~]").expect("c1 csi regex"));

impl Default for Redactor {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Redactor {
    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    pub fn with_enabled(enabled: bool) -> Self {
        Self { enabled }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn redact(&self, input: &str) -> String {
        if !self.enabled {
            return input.to_string();
        }
        let mut output = input.to_string();
        for pattern in PATTERNS.iter() {
            output = pattern
                .regex
                .replace_all(&output, pattern.replacement)
                .into_owned();
        }
        output
    }

    pub fn redact_json(&self, value: &mut Value) {
        if !self.enabled {
            return;
        }
        match value {
            Value::String(text) => *text = self.redact(text),
            Value::Array(items) => {
                for item in items {
                    self.redact_json(item);
                }
            }
            Value::Object(map) => {
                for item in map.values_mut() {
                    self.redact_json(item);
                }
            }
            _ => {}
        }
    }
}

/// Remove terminal control sequences and non-printable controls. Tool output
/// is untrusted and must never be able to drive the user's terminal.
pub fn strip_ansi(input: &str) -> String {
    let without_osc = ANSI_OSC.replace_all(input, "");
    let without_csi = ANSI_CSI.replace_all(&without_osc, "");
    let without_c1 = C1_CSI.replace_all(&without_csi, "");
    ANSI_SINGLE.replace_all(&without_c1, "").into_owned()
}

pub fn sanitize_text(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    strip_ansi(&normalized)
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect()
}

pub fn truncate_utf8(input: &str, max_bytes: usize) -> (&str, bool) {
    if input.len() <= max_bytes {
        return (input, false);
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (&input[..end], true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_common_credentials() {
        let redactor = Redactor::default();
        let input = "OPENAI_API_KEY=sk-1234567890abcdefghijklmnop\n\
                     token: ghp_1234567890abcdefghijklmnop\n\
                     password = \"hunter2hunter2\"\n\
                     -----BEGIN RSA PRIVATE KEY-----\nsecret\n-----END RSA PRIVATE KEY-----";
        let output = redactor.redact(input);
        assert!(!output.contains("sk-1234567890"));
        assert!(!output.contains("ghp_1234567890"));
        assert!(!output.contains("hunter2hunter2"));
        assert!(!output.contains("secret\n-----END RSA"));
    }

    #[test]
    fn redacts_json_values_recursively() {
        let redactor = Redactor::default();
        let mut value = json!({"outer": {"key": "sk-1234567890abcdefghijklmnop"}});
        redactor.redact_json(&mut value);
        let rendered = value.to_string();
        assert!(!rendered.contains("sk-1234567890"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn strips_terminal_escapes() {
        let input = "hello\x1b[31m red\x1b[0m \x1b]0;title\x07world";
        assert_eq!(strip_ansi(input), "hello red world");
    }

    #[test]
    fn truncates_on_char_boundary() {
        let (text, truncated) = truncate_utf8("ééé", 3);
        assert!(truncated);
        assert_eq!(text, "é");
    }
}

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// Best-effort secret redaction applied before content reaches a model, the
/// event log, or the terminal. This is a mitigation, not a proof: new secret
/// formats need new patterns. Entropy-based detection covers candidate strings
/// that match no known format; see [`crate::entropy`] for what it does and does
/// not catch.
#[derive(Debug, Clone)]
pub struct Redactor {
    enabled: bool,
    /// Run the entropy pass. On by default; can be turned off without disabling
    /// pattern redaction.
    entropy: bool,
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

    // key = value / key: value forms. The key list is deliberately broad:
    // matching a credential-sounding key redacts the whole value, whatever its
    // format, which is what catches vendor-specific formats this file has never
    // heard of.
    add(
        r#"(?i)(api[_-]?key|apikey|access[_-]?key|access[_-]?token|auth[_-]?token|bearer[_-]?token|client[_-]?secret|secret[_-]?key|token|secret|password|passwd|pwd|credential|private[_-]?key|signature|session[_-]?key|cookie)(\s*[:=]\s*["']?)([A-Za-z0-9_\-./+=]{8,})"#,
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
    add(r"(?i)(https?://[^\s:/]+:)[^@\s]+@", "${1}[REDACTED]@");

    patterns
});

static ANSI_CSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("ansi csi regex"));
static ANSI_OSC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").expect("ansi osc regex"));
static ANSI_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b[@-_]").expect("ansi single regex"));
static C1_CSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\u{9b}[0-?]*[ -/]*[@-~]").expect("c1 csi regex"));

/// A run of credential-plausible characters: base64 plus the punctuation that
/// shows up in API material. Quotes, colons, and brackets are excluded so that
/// JSON punctuation, URLs, and the redaction markers themselves never match.
static BARE_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/_.=-]{32,}").expect("bare token regex"));

const HIGH_ENTROPY_MARKER: &str = "[HIGH-ENTROPY]";

/// Replace candidate strings that match no known pattern but are too random to
/// be identifiers or prose.
///
/// This runs after the known formats, so anything already replaced by a marker
/// is no longer a candidate. Hits are marked distinctly from known secrets:
/// a reader can tell a heuristic match (possibly a false positive) from a
/// recognised credential.
fn redact_high_entropy(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut last = 0;

    for candidate in BARE_TOKEN.find_iter(input) {
        let text = candidate.as_str();
        if crate::entropy::looks_like_secret_material(text) {
            output.push_str(&input[last..candidate.start()]);
            output.push_str(HIGH_ENTROPY_MARKER);
            last = candidate.end();
        }
    }
    output.push_str(&input[last..]);
    output
}

impl Default for Redactor {
    fn default() -> Self {
        Self {
            enabled: true,
            entropy: true,
        }
    }
}

impl Redactor {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            entropy: false,
        }
    }

    pub fn with_enabled(enabled: bool) -> Self {
        Self {
            enabled,
            entropy: enabled,
        }
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
        if self.entropy {
            output = redact_high_entropy(&output);
        }
        output
    }

    /// Turn the entropy pass off while keeping pattern redaction. Useful when
    /// reading source full of base64 fixtures where the heuristic produces
    /// noise.
    pub fn with_entropy(mut self, entropy: bool) -> Self {
        self.entropy = entropy;
        self
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
    fn redacts_a_vendor_format_no_pattern_knows() {
        // No recognised prefix: only the key name and the value's randomness.
        let redactor = Redactor::default();
        let input = "NVIDIA_API_TOKEN = \"Tn4pXq8Rw2Zb6Ky1Ms7Hd3Lg9Cf5Vj0Qe\"";
        let output = redactor.redact(input);
        assert!(
            !output.contains("Tn4pXq8Rw2Zb6Ky1Ms7Hd3Lg9Cf5Vj0Qe"),
            "the token leaked: {output}"
        );
        assert!(output.contains("[REDACTED]") || output.contains("[HIGH-ENTROPY]"));
    }

    #[test]
    fn redacts_a_bare_token_with_no_key_name_at_all() {
        let redactor = Redactor::default();
        let input = "found in scratch.txt: aG7kQ9xZ3mLp2wR5tB8nV4cJ1hD6sF0uY";
        let output = redactor.redact(input);
        assert!(
            !output.contains("aG7kQ9xZ3mLp2wR5tB8nV4cJ1hD6sF0uY"),
            "the bare token leaked: {output}"
        );
    }

    #[test]
    fn entropy_pass_keeps_identifiers_and_hashes() {
        // These are the false positives that would make the agent useless if
        // the heuristic were loose, so each is asserted to survive.
        let redactor = Redactor::default();
        let samples = [
            "commit 0123456789abcdef0123456789abcdef01234567",
            "sha256 e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "request id 550e8400-e29b-41d4-a716-446655440000",
            "async fn getUserProfileByAccountIdentifier",
            "https://example.com/path/to/the/configuration/resource",
        ];
        for sample in samples {
            let output = redactor.redact(sample);
            assert_eq!(
                output, sample,
                "redaction destroyed legitimate content in: {sample}"
            );
        }
    }

    #[test]
    fn entropy_pass_can_be_disabled_without_losing_patterns() {
        // With a key name present, both passes agree; use a bare token to
        // isolate what the entropy pass actually contributes.
        let bare = "aG7kQ9xZ3mLp2wR5tB8nV4cJ1hD6sF0uY";

        let with_entropy = Redactor::default().redact(bare);
        assert!(
            with_entropy.contains("[HIGH-ENTROPY]"),
            "the entropy pass should mask a bare token: {with_entropy}"
        );

        let without = Redactor::default().with_entropy(false).redact(bare);
        assert_eq!(without, bare, "disabling entropy must leave it untouched");

        let keyed = "token = \"aG7kQ9xZ3mLp2wR5tB8nV4cJ1hD6sF0uY\"";
        assert!(
            Redactor::default()
                .with_entropy(false)
                .redact(keyed)
                .contains("[REDACTED]"),
            "key-based redaction must survive with entropy off"
        );
    }

    #[test]
    fn markers_are_not_redacted_again() {
        // The redaction markers must not be re-matched as candidates, or output
        // would lose the marker itself.
        let redactor = Redactor::default();
        let once = redactor.redact("token = aG7kQ9xZ3mLp2wR5tB8nV4cJ1hD6sF0uY");
        let twice = redactor.redact(&once);
        assert_eq!(once, twice, "redaction is not idempotent");
    }

    #[test]
    fn truncates_on_char_boundary() {
        let (text, truncated) = truncate_utf8("ééé", 3);
        assert!(truncated);
        assert_eq!(text, "é");
    }
}

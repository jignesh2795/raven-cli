//! Entropy-based secret detection.
//!
//! Pattern redaction only catches secret formats it has seen. This module adds
//! a heuristic for the rest: a string that is too random to be natural language
//! or an identifier is probably credential material.
//!
//! The gate is deliberately conservative. Redacting indiscriminately would
//! destroy legitimate content — git SHAs, UUIDs, base64 blobs in fixtures, and
//! long identifiers in source are all high-entropy, and a coding agent that
//! cannot see them stops being useful. So a candidate is only redacted when it
//! is long, uses several character classes, is not all-hex, and is not shaped
//! like a UUID. Hits are marked distinctly from known formats so a reader can
//! tell a heuristic match from a recognised secret.

/// Minimum candidate length. Shorter strings are too common in real text.
const MIN_LEN: usize = 32;

/// Shannon entropy, in bits per character, below which a string is not
/// considered credential material. Natural language sits near 4.0 with heavy
/// repetition; random base64 sits near 5.5-6.0.
const MIN_ENTROPY: f64 = 4.6;

pub fn shannon_entropy(input: &str) -> f64 {
    if input.is_empty() {
        return 0.0;
    }

    let mut counts = [0usize; 256];
    for byte in input.bytes() {
        counts[byte as usize] += 1;
    }

    let len = input.len() as f64;
    counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let probability = *count as f64 / len;
            -probability * probability.log2()
        })
        .sum()
}

/// True when `candidate` looks like credential material rather than an
/// identifier, a hash, or natural text.
pub fn looks_like_secret_material(candidate: &str) -> bool {
    if candidate.len() < MIN_LEN {
        return false;
    }

    // Identifiers in practice: git SHAs, content hashes, checksums.
    if is_all_hex(candidate) {
        return false;
    }
    // Identifiers by construction.
    if is_uuid_shaped(candidate) {
        return false;
    }
    // Repeated filler has high length but no information.
    let distinct = distinct_character_count(candidate);
    if distinct * 8 < candidate.len() {
        return false;
    }

    let classes = character_class_count(candidate);
    if classes < 2 {
        return false;
    }

    shannon_entropy(candidate) >= MIN_ENTROPY
}

fn is_all_hex(candidate: &str) -> bool {
    !candidate.is_empty() && candidate.chars().all(|c| c.is_ascii_hexdigit() || c == '_')
}

fn is_uuid_shaped(candidate: &str) -> bool {
    let parts: Vec<&str> = candidate.split('-').collect();
    parts.len() == 5
        && parts[0].len() == 8
        && parts[1].len() == 4
        && parts[2].len() == 4
        && parts[3].len() == 4
        && parts[4].len() == 12
}

fn distinct_character_count(candidate: &str) -> usize {
    let mut seen = [false; 256];
    for byte in candidate.bytes() {
        seen[byte as usize] = true;
    }
    seen.iter().filter(|present| **present).count()
}

fn character_class_count(candidate: &str) -> usize {
    let mut classes = 0;
    if candidate.chars().any(|c| c.is_ascii_lowercase()) {
        classes += 1;
    }
    if candidate.chars().any(|c| c.is_ascii_uppercase()) {
        classes += 1;
    }
    if candidate.chars().any(|c| c.is_ascii_digit()) {
        classes += 1;
    }
    if candidate
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && !c.is_whitespace())
    {
        classes += 1;
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_rises_with_randomness() {
        assert!(shannon_entropy("aaaa") < shannon_entropy("abcd"));
        assert!(shannon_entropy("").abs() < f64::EPSILON);
        // English text is far below random base64.
        let prose = "the quick brown fox jumps over the lazy dog again";
        assert!(shannon_entropy(prose) < MIN_ENTROPY);
    }

    #[test]
    fn accepts_random_credential_material() {
        // base64-ish random strings of the kind vendors issue.
        let samples = [
            "aG7kQ9xZ3mLp2wR5tB8nV4cJ1hD6sF0uY",
            "Tn4pXq8Rw2Zb6Ky1Ms7Hd3Lg9Cf5Vj0Qe",
            "9fKp2Xq7Rm4Wz8Lb1Nc6Th3Yd5Sj0Gv9P",
        ];
        for sample in samples {
            assert!(
                looks_like_secret_material(sample),
                "expected {sample} to look like credential material"
            );
        }
    }

    #[test]
    fn rejects_identifiers_hashes_and_prose() {
        let not_secrets = [
            "0123456789abcdef0123456789abcdef01234567", // git SHA-1
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", // sha256
            "550e8400-e29b-41d4-a716-446655440000",     // UUID
            "https://example.com/path/to/resource",     // URL, not random
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",     // repeated filler
            "getUserProfileByAccountIdentifier",        // identifier
            "the configuration value for the retry limit",
        ];
        for sample in not_secrets {
            assert!(
                !looks_like_secret_material(sample),
                "did not expect {sample} to be treated as a secret"
            );
        }
    }

    #[test]
    fn short_strings_are_never_flagged() {
        assert!(!looks_like_secret_material("aB3"));
        assert!(!looks_like_secret_material(""));
    }
}

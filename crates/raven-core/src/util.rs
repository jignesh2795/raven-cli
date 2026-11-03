//! Small shared helpers that have no better home.

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_bytes_deterministically() {
        assert_eq!(sha256_hex(b"").len(), 64);
        assert_ne!(sha256_hex(b"a"), sha256_hex(b"b"));
        assert_eq!(sha256_hex(b"raven"), hex::encode(Sha256::digest(b"raven")));
    }
}

//! The node token: the bearer every `/v1/swarm` route requires, derived on each device
//! from the per-account secret the auth worker issues alongside the mesh join key
//! (`POST /v1/mesh/join-key` → `nodeSecret`, a stable 32-byte hex value per account).
//!
//! Same account ⇒ same secret ⇒ the same token on every node, which is what lets a peer
//! authenticate with no exchange; a different account can never derive it. The token is
//! HMAC-SHA256 over a fixed context string keyed by the secret, hex-encoded: the secret
//! itself never rides on the wire, and the context versions the derivation so a future
//! rotation cannot collide with today's tokens.
//!
//! This replaces the earlier HMAC-of-the-account-email scheme, which anyone who knew the
//! address could reproduce — an account TAG, not a credential.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// The HMAC message. Versioned so a v2 derivation can never equal a v1 token.
pub const NODE_TOKEN_CONTEXT: &[u8] = b"leanzero-link/v1/node-token";

/// `hex(HMAC-SHA256(key = secret bytes, msg = NODE_TOKEN_CONTEXT))` — 64 lowercase hex
/// chars. The caller guarantees a non-empty secret (`LinkManager::connect` refuses an
/// absent one loudly); the derivation itself accepts any length.
pub fn node_token_from_secret(secret: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(NODE_TOKEN_CONTEXT);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors computed independently with Python's `hmac`/`hashlib`
    /// (`hmac.new(secret, b"leanzero-link/v1/node-token", hashlib.sha256).hexdigest()`),
    /// so a wrong key/message order or a wrong context here fails against an oracle that
    /// is not this code.
    #[test]
    fn derivation_matches_an_independent_hmac_oracle() {
        assert_eq!(
            node_token_from_secret(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            ),
            "2ceb6b570d58cb6ae217bc158e6f5c806128dd7f49f35557dfe737c79c1dacc9"
        );
        assert_eq!(
            node_token_from_secret("test-secret"),
            "f4dcfb9d9d4d2443d9cc952f45c3d19988a6c943fce9e17a1197439956de18b8"
        );
    }

    #[test]
    fn different_secrets_never_share_a_token() {
        let a = node_token_from_secret("test-secret");
        let b = node_token_from_secret("other-secret");
        assert_ne!(a, b);
        assert_eq!(
            b,
            "c4b5260384e3cca827a1fb9a8ecd438faea9fe2151343fafb70fe4ff2c0f440e"
        );
        assert_eq!(a.len(), 64);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    /// The old email-keyed scheme must not survive in disguise: keying by an address
    /// yields a different value than keying by a secret, and nothing here accepts an
    /// email at all.
    #[test]
    fn token_is_not_the_email_derived_value() {
        assert_ne!(
            node_token_from_secret("test-secret"),
            "a7ef46125747101362f899c12066596540946e08e7147df0e119e27c15c74516",
            "a7ef… is HMAC(key=\"a@example.com\") — the tag scheme this replaced"
        );
    }
}

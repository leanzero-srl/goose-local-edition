//! The callback signature: `x-cognirunner-signature: sha256=<hex(HMAC-SHA256(secret, body))>`
//! over the RAW request body bytes, exactly as sent. The receiver (CogniRunner's Forge web
//! trigger) verifies the same construction over the bytes it received.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

pub const SIGNATURE_HEADER: &str = "x-cognirunner-signature";
const PREFIX: &str = "sha256=";

pub fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(body);
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{PREFIX}{hex}")
}

/// Constant-time comparison of a presented header value against the expected signature.
pub fn verify(secret: &str, body: &[u8], presented: &str) -> bool {
    let expected = sign(secret, body);
    bool::from(expected.as_bytes().ct_eq(presented.trim().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vector computed independently with Python's `hmac`/`hashlib`:
    /// `hmac.new(b"cb-secret", b'{"a":1}', hashlib.sha256).hexdigest()`.
    #[test]
    fn signs_the_raw_body_with_the_secret() {
        assert_eq!(
            sign("cb-secret", br#"{"a":1}"#),
            "sha256=5bfcf269c5114f8c446817c6bce7d9ba989c7814cd6432435cff88a0c0431816"
        );
    }

    #[test]
    fn verify_accepts_the_right_signature_and_refuses_everything_else() {
        let body = br#"{"taskId":"t1","seq":1}"#;
        let good = sign("s", body);
        assert!(verify("s", body, &good));
        assert!(verify("s", body, &format!("  {good} ")));
        assert!(!verify("other", body, &good));
        assert!(!verify("s", br#"{"taskId":"t1","seq":2}"#, &good));
        assert!(!verify("s", body, "sha256=00"));
        assert!(!verify("s", body, ""));
    }
}

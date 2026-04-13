// OAuth Authorization Server helpers.
//
// This module provides support functions for Hiveloom acting as an OAuth
// resource that issues tokens (used by the MCP surface). The actual AS
// endpoints live in `server::mcp::auth`.

use sha2::{Digest, Sha256};

/// Hash a token for storage (SHA-256 hex).
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Generate a cryptographically random opaque token (UUID v4 hex, no dashes).
pub fn generate_token() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "")
}

/// Verify a PKCE S256 code challenge.
///
/// Computes `BASE64URL_NO_PAD(SHA256(code_verifier))` and compares to the
/// stored `code_challenge`.
pub fn verify_pkce(code_verifier: &str, code_challenge: &str) -> bool {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let hash = Sha256::digest(code_verifier.as_bytes());
    let computed = URL_SAFE_NO_PAD.encode(hash);
    computed == code_challenge
}

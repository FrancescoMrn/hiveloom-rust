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

//! API key authentication middleware.
//!
//! Mirrors `security.py`: reads `DASHBOARD_API_KEY` from env, generates a
//! random key if missing, and validates Bearer tokens on protected routes.

use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use dream_common::error::AppError;
use tracing::warn;

use crate::state::AppState;

/// Axum middleware that validates the Bearer token against `AppState.api_key`.
///
/// Used as a layer on protected route groups:
/// ```ignore
/// router.layer(axum::middleware::from_fn_with_state(state, require_api_key))
/// ```
pub async fn require_api_key(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let token = match auth_header.as_deref() {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => return Err(AppError::Unauthorized),
    };

    if !constant_time_eq(token.as_bytes(), state.api_key.as_bytes()) {
        return Err(AppError::Forbidden);
    }

    Ok(next.run(req).await)
}

/// Constant-time comparison to prevent timing attacks (mirrors `secrets.compare_digest`).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Generate the API key at startup. Reads from env or generates a random one.
pub fn resolve_api_key() -> String {
    if let Ok(key) = std::env::var("DASHBOARD_API_KEY") {
        if !key.is_empty() {
            return key;
        }
    }

    use rand::Rng;
    let key: String = rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(43) // ~256 bits, matches Python's token_urlsafe(32)
        .map(char::from)
        .collect();

    let key_file = std::path::Path::new("/data/dashboard-api-key.txt");
    if let Some(parent) = key_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(key_file, &key) {
        warn!("Failed to write API key file: {e}");
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(key_file, std::fs::Permissions::from_mode(0o600));
        }
        warn!(
            "DASHBOARD_API_KEY not set. Generated temporary key and wrote to {} (mode 0600). \
             Set DASHBOARD_API_KEY in your .env file for production.",
            key_file.display()
        );
    }

    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq_same_bytes() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn test_constant_time_eq_different_bytes_same_length() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer_string"));
    }

    #[test]
    fn test_constant_time_eq_both_empty() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_constant_time_eq_one_empty_one_not() {
        assert!(!constant_time_eq(b"", b"notempty"));
    }
}

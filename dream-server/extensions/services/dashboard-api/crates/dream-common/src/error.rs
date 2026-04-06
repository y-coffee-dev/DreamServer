//! Shared error types for the Dashboard API.
//!
//! `AppError` maps to FastAPI's `HTTPException` — every variant produces
//! a JSON response with `{"detail": "..."}` matching the Python API contract.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    #[error("Authentication required. Provide Bearer token in Authorization header.")]
    Unauthorized,

    #[error("Invalid API key.")]
    Forbidden,

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    ServiceUnavailable(String),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, detail) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            AppError::Internal(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal error: {e}"),
            ),
        };

        let body = json!({ "detail": detail });

        let mut resp = axum::Json(body).into_response();
        *resp.status_mut() = status;
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn test_bad_request() {
        let resp = AppError::BadRequest("invalid input".into()).into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["detail"], "invalid input");
    }

    #[tokio::test]
    async fn test_unauthorized() {
        let resp = AppError::Unauthorized.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["detail"],
            "Authentication required. Provide Bearer token in Authorization header."
        );
    }

    #[tokio::test]
    async fn test_forbidden() {
        let resp = AppError::Forbidden.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["detail"], "Invalid API key.");
    }

    #[tokio::test]
    async fn test_not_found() {
        let resp = AppError::NotFound("service xyz not found".into()).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["detail"], "service xyz not found");
    }

    #[tokio::test]
    async fn test_service_unavailable() {
        let resp = AppError::ServiceUnavailable("backend down".into()).into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["detail"], "backend down");
    }

    #[tokio::test]
    async fn test_internal_error() {
        let err = anyhow::anyhow!("something broke");
        let resp = AppError::Internal(err).into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["detail"], "Internal error: something broke");
    }
}

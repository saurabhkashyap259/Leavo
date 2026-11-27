//! HTTP error responses: `{"error": {"code", "message", "details"?}}` with the catalogue status.
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use leavo_core::error::{DomainError, ErrorCode};
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: Option<String>,
    pub details: Option<serde_json::Value>,
    /// Seconds for a `Retry-After` header (rate limits and locks).
    pub retry_after: Option<u64>,
}

impl ApiError {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            message: None,
            details: None,
            retry_after: None,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn retry_after(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    /// Log an unexpected failure and return a generic 500 (never leaks internals).
    pub fn internal(err: impl std::fmt::Display) -> Self {
        tracing::error!(error = %err, "internal error");
        Self::new(ErrorCode::Internal)
    }
}

impl From<ErrorCode> for ApiError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

impl From<DomainError> for ApiError {
    fn from(e: DomainError) -> Self {
        Self {
            code: e.code,
            message: None,
            details: e.details,
            retry_after: None,
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => Self::new(ErrorCode::NotFound),
            other => Self::internal(other),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let message = self
            .message
            .unwrap_or_else(|| self.code.default_message().to_string());
        let mut error = json!({ "code": self.code.as_str(), "message": message });
        if let Some(details) = self.details {
            error["details"] = details;
        }
        let mut resp = (status, Json(json!({ "error": error }))).into_response();
        if let Some(secs) = self.retry_after {
            resp.headers_mut()
                .insert(axum::http::header::RETRY_AFTER, secs.into());
        }
        resp
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// `422 VALIDATION` naming the offending field (`details.field`).
pub fn field_error(field: &str) -> ApiError {
    ApiError::new(ErrorCode::Validation).with_details(json!({ "field": field }))
}

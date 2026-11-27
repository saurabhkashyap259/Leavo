//! CSRF defence: SameSite=Lax cookies plus a required `X-Requested-With: leavo` header on every
//! state-changing request (research R4). Browsers cannot add custom headers cross-site without CORS.
use crate::error::ApiError;
use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use leavo_core::error::ErrorCode;

pub const HEADER: &str = "x-requested-with";
pub const VALUE: &str = "leavo";

pub async fn layer(req: Request, next: Next) -> Response {
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let ok = safe || req.headers().get(HEADER).and_then(|v| v.to_str().ok()) == Some(VALUE);
    if ok {
        next.run(req).await
    } else {
        ApiError::new(ErrorCode::Csrf).into_response()
    }
}

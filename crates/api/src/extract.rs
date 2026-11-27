//! JSON body extractor whose rejections use the standard error envelope (`VALIDATION`).
use crate::error::ApiError;
use axum::extract::{FromRequest, Request};
use leavo_core::error::ErrorCode;
use serde::de::DeserializeOwned;
use serde_json::json;

pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(v)) => Ok(ApiJson(v)),
            Err(rej) => Err(ApiError::new(ErrorCode::Validation)
                .with_details(json!({ "body": rej.body_text() }))),
        }
    }
}

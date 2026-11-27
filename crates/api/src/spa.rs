//! Serves the web app embedded at build time from `web/dist` (T050). Client-side routes fall
//! back to `index.html`; `/api/*` never does (unknown API paths stay JSON 404s).
use crate::error::ApiError;
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use leavo_core::error::ErrorCode;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
#[allow_missing = true]
struct WebAssets;

#[derive(Debug, PartialEq, Eq)]
pub enum Resolved {
    Asset(String),
    Index,
    NotFound,
}

/// Decide what to serve for `path` given which asset files exist.
pub fn resolve(path: &str, exists: impl Fn(&str) -> bool) -> Resolved {
    let rel = path.trim_start_matches('/');
    if rel.starts_with("api/") || rel == "api" {
        return Resolved::NotFound;
    }
    if !rel.is_empty() && exists(rel) {
        return Resolved::Asset(rel.to_string());
    }
    let looks_like_file = rel
        .rsplit('/')
        .next()
        .is_some_and(|last| last.contains('.'));
    if looks_like_file || !exists("index.html") {
        Resolved::NotFound
    } else {
        Resolved::Index
    }
}

pub async fn serve(uri: Uri) -> Response {
    match resolve(uri.path(), |p| WebAssets::get(p).is_some()) {
        Resolved::Asset(p) => file(&p, true),
        Resolved::Index => file("index.html", false),
        Resolved::NotFound => ApiError::new(ErrorCode::NotFound).into_response(),
    }
}

fn file(path: &str, immutable: bool) -> Response {
    let Some(asset) = WebAssets::get(path) else {
        return ApiError::new(ErrorCode::NotFound).into_response();
    };
    let mime = asset.metadata.mimetype().to_string();
    let cache = if immutable && path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut resp = (StatusCode::OK, asset.data.into_owned()).into_response();
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&mime) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exists(p: &str) -> bool {
        matches!(p, "index.html" | "assets/app-123.js" | "favicon.svg")
    }

    #[test]
    fn serves_existing_assets() {
        assert_eq!(
            resolve("/assets/app-123.js", exists),
            Resolved::Asset("assets/app-123.js".into())
        );
        assert_eq!(
            resolve("/favicon.svg", exists),
            Resolved::Asset("favicon.svg".into())
        );
    }

    #[test]
    fn client_routes_fall_back_to_index() {
        assert_eq!(resolve("/", exists), Resolved::Index);
        assert_eq!(resolve("/dashboard", exists), Resolved::Index);
        assert_eq!(resolve("/admin/policy", exists), Resolved::Index);
    }

    #[test]
    fn missing_files_and_api_paths_are_not_found() {
        assert_eq!(resolve("/assets/missing.js", exists), Resolved::NotFound);
        assert_eq!(resolve("/api/v1/nope", exists), Resolved::NotFound);
        assert_eq!(
            resolve("/dashboard", |_| false),
            Resolved::NotFound,
            "no build embedded"
        );
    }
}

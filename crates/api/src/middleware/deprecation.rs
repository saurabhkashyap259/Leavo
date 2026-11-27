//! Adds `Deprecation: true` to responses of deprecated operations (FR-050). Operations are
//! listed here when deprecated and removed only in the next major version.
use axum::extract::Request;
use axum::http::{HeaderValue, Method};
use axum::middleware::Next;
use axum::response::Response;

/// (method, path) of deprecated `/api/v1` operations. Empty in v1.0.
pub const DEPRECATED: &[(&str, &str)] = &[];

pub fn is_deprecated(list: &[(&str, &str)], method: &Method, path: &str) -> bool {
    list.iter()
        .any(|(m, p)| *m == method.as_str() && *p == path)
}

pub async fn layer(req: Request, next: Next) -> Response {
    let deprecated = is_deprecated(DEPRECATED, req.method(), req.uri().path());
    let mut resp = next.run(req).await;
    if deprecated {
        resp.headers_mut()
            .insert("deprecation", HeaderValue::from_static("true"));
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_method_and_path() {
        let list = [("GET", "/api/v1/old")];
        assert!(is_deprecated(&list, &Method::GET, "/api/v1/old"));
        assert!(!is_deprecated(&list, &Method::POST, "/api/v1/old"));
        assert!(!is_deprecated(&list, &Method::GET, "/api/v1/new"));
    }
}

//! T016 — every error code serialises to the catalogue shape and HTTP status (data-model.md).
use axum::body::to_bytes;
use axum::response::IntoResponse;
use leavo_api::error::ApiError;
use leavo_core::error::ErrorCode;

async fn render(err: ApiError) -> (u16, serde_json::Value) {
    let resp = err.into_response();
    let status = resp.status().as_u16();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn every_code_has_shape_status_and_message() {
    assert_eq!(
        ErrorCode::ALL.len(),
        44,
        "must match data-model.md error code catalogue"
    );
    for code in ErrorCode::ALL {
        let (status, body) = render(ApiError::new(*code)).await;
        assert_eq!(status, code.http_status(), "{code:?}");
        assert_eq!(body["error"]["code"], code.as_str(), "{code:?}");
        assert!(
            !body["error"]["message"].as_str().unwrap().is_empty(),
            "{code:?}"
        );
        assert!(
            body["error"].get("details").is_none(),
            "no details unless given"
        );
    }
}

#[tokio::test]
async fn statuses_match_catalogue() {
    use ErrorCode::*;
    let expect = [
        (InsufficientBalance, 422),
        (SelfApproval, 403),
        (NotEligibleApprover, 403),
        (AlreadyDecided, 409),
        (LeaveYearLocked, 409),
        (DomainTaken, 409),
        (Unauthenticated, 401),
        (Csrf, 403),
        (NotFound, 404),
        (Locked, 429),
        (RateLimited, 429),
        (IdempotencyMismatch, 409),
        (FileTooLarge, 413),
        (FileTypeNotAllowed, 415),
        (Internal, 500),
    ];
    for (code, status) in expect {
        assert_eq!(code.http_status(), status, "{code:?}");
    }
}

#[tokio::test]
async fn codes_are_screaming_snake_case_and_unique() {
    let mut seen = std::collections::HashSet::new();
    for code in ErrorCode::ALL {
        let s = code.as_str();
        assert!(s.chars().all(|c| c.is_ascii_uppercase() || c == '_'), "{s}");
        assert!(seen.insert(s), "duplicate {s}");
    }
}

#[tokio::test]
async fn details_and_custom_message_are_included() {
    let err = ApiError::new(ErrorCode::AlreadyDecided)
        .with_message("Approved by Priya on 3 Dec")
        .with_details(serde_json::json!({"decided_by": "Priya"}));
    let (status, body) = render(err).await;
    assert_eq!(status, 409);
    assert_eq!(body["error"]["message"], "Approved by Priya on 3 Dec");
    assert_eq!(body["error"]["details"]["decided_by"], "Priya");
}

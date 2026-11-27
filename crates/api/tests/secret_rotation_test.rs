//! T207 — restarting with a new `LEAVO_SECRET_KEY` (FR-063) signs everyone out and voids unused
//! sign-in codes and email action links. Restarting again with the same key changes nothing.
mod common;

use common::factories::*;
use common::*;
use leavo_api::app::{router, AppState};
use leavo_api::startup::apply_secret_rotation;
use leavo_email::Templates;
use leavo_storage::Storage;
use rust_decimal_macros::dec;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;

const NEW_SECRET: &str = "rotated-rotated-rotated-rotated-rotated-rotated-rotated-rotated-99";

/// The same database, served by a second instance configured with `secret`.
async fn restart(app: &TestApp, secret: &str) -> String {
    let state = AppState::new(
        Arc::new(test_config(&app.db.url, &[("LEAVO_SECRET_KEY", secret)])),
        app.db.app.clone(),
        app.db.owner.clone(),
        app.clock.clone(),
        app.email.clone(),
        Arc::new(Templates::new()),
        Storage::memory(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn a_new_key_signs_everyone_out_and_voids_codes_and_links() {
    let app = TestApp::spawn().await;
    // First start: the key is remembered, nothing is revoked.
    assert!(!apply_secret_rotation(&app.db.owner, SECRET, t0())
        .await
        .unwrap());
    let (org, admin_user, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let asha_c = app.login_as(&asha).await;
    submit(&app, &asha_c, &leave_body(cl, "2026-11-30", "2026-11-30")).await;
    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": "admin@acme.test", "purpose": "login" }),
    )
    .await;
    assert_eq!(status, 202);
    let codes = codes_sent_to(&app, "admin@acme.test").await;
    let code = codes.last().expect("a login code").clone();
    let action = app
        .email
        .sent()
        .into_iter()
        .find(|m| m.to == "admin@acme.test" && m.text.contains("/api/v1/approvals/action/"))
        .expect("an action email");
    let action_path = action
        .text
        .split_whitespace()
        .find(|w| w.contains("/api/v1/approvals/action/"))
        .unwrap()
        .trim_start_matches("http://leavo.test")
        .to_string();
    let (status, _) = get_json(&admin, app.url("/api/v1/me")).await;
    assert_eq!(status, 200);

    // Restart with a new key.
    assert!(apply_secret_rotation(&app.db.owner, NEW_SECRET, t0())
        .await
        .unwrap());
    let base = restart(&app, NEW_SECRET).await;
    let at = |p: &str| format!("{base}{p}");

    let (status, _) = get_json(&admin, at("/api/v1/me")).await;
    assert_eq!(status, 401, "sessions end");
    let (status, out) = post_json(
        &app.anon(),
        at("/api/v1/auth/verify"),
        json!({ "email": "admin@acme.test", "code": code, "purpose": "login" }),
    )
    .await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["code"], "CODE_INVALID");
    let res = app.anon().get(at(&action_path)).send().await.unwrap();
    assert_eq!(res.status(), 303);
    let location = res.headers()["location"].to_str().unwrap().to_string();
    assert!(location.starts_with("/approvals?link="), "{location}");
    assert!(!location.contains("request="), "{location}");

    // Signing in again works, and restarting with the same key keeps the new session.
    let fresh = app.with_token(&create_session(&app, &admin_user, false).await);
    assert!(!apply_secret_rotation(&app.db.owner, NEW_SECRET, t0())
        .await
        .unwrap());
    let (status, _) = get_json(&fresh, at("/api/v1/me")).await;
    assert_eq!(status, 200);
}

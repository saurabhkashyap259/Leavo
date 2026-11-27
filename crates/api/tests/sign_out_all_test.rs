//! T181 — an admin signs everyone in the organization out (FR-063).
mod common;

use common::factories::*;
use common::*;
use serde_json::json;

#[tokio::test]
async fn every_session_in_the_org_ends_including_the_callers() {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let (_, _, other_admin) = org_with_admin(&app, "other.test").await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let asha_1 = app.login_as(&asha).await;
    let asha_2 = app.login_as(&asha).await;

    let (status, _) = post_json(
        &asha_1,
        app.url("/api/v1/admin/org/sign-out-all"),
        json!({}),
    )
    .await;
    assert_eq!(status, 403);

    let (status, _) = post_json(&admin, app.url("/api/v1/admin/org/sign-out-all"), json!({})).await;
    assert_eq!(status, 204);
    for client in [&admin, &asha_1, &asha_2] {
        let (status, _) = get_json(client, app.url("/api/v1/me")).await;
        assert_eq!(status, 401);
    }
    // Another organization on the same instance is untouched.
    let (status, _) = get_json(&other_admin, app.url("/api/v1/me")).await;
    assert_eq!(status, 200);
    // People can sign in again.
    let (status, _) = get_json(&app.login_as(&asha).await, app.url("/api/v1/me")).await;
    assert_eq!(status, 200);
}

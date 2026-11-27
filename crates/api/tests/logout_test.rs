//! T057 — POST /auth/logout (FR-005).
mod common;
use common::factories::{create_org, create_session, create_user, NewUser};
use common::TestApp;

#[tokio::test]
async fn logout_revokes_only_the_current_session() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let laptop = app.with_token(&create_session(&app, &user, false).await);
    let phone = app.with_token(&create_session(&app, &user, false).await);

    let res = laptop
        .post(app.url("/api/v1/auth/logout"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    let cookie = res.headers()["set-cookie"].to_str().unwrap();
    assert!(
        cookie.starts_with("leavo_sid=") && cookie.contains("Max-Age=0"),
        "{cookie}"
    );

    assert_eq!(
        laptop
            .get(app.url("/api/v1/me"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        phone
            .get(app.url("/api/v1/me"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn logout_all_revokes_every_session_of_the_user() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let other = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    let laptop = app.with_token(&create_session(&app, &user, false).await);
    let phone = app.with_token(&create_session(&app, &user, true).await);
    let ravi = app.login_as(&other).await;

    assert_eq!(
        laptop
            .post(app.url("/api/v1/auth/logout?all=true"))
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        laptop
            .get(app.url("/api/v1/me"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        phone
            .get(app.url("/api/v1/me"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        ravi.get(app.url("/api/v1/me"))
            .send()
            .await
            .unwrap()
            .status(),
        200,
        "other users unaffected"
    );
}

#[tokio::test]
async fn logout_without_session_is_unauthenticated() {
    let app = TestApp::spawn().await;
    assert_eq!(
        app.anon()
            .post(app.url("/api/v1/auth/logout"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}

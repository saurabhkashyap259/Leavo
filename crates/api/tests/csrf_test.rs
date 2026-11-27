//! T022 — CSRF: state-changing requests must carry `X-Requested-With: leavo` (research R4).
mod common;
use common::TestApp;

#[tokio::test]
async fn post_without_header_is_rejected() {
    let app = TestApp::spawn().await;
    let res = reqwest::Client::new()
        .post(app.url("/api/v1/auth/logout"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "CSRF");
}

#[tokio::test]
async fn post_with_wrong_header_value_is_rejected() {
    let app = TestApp::spawn().await;
    let res = reqwest::Client::new()
        .post(app.url("/api/v1/auth/logout"))
        .header("x-requested-with", "XMLHttpRequest")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn post_with_header_passes_csrf() {
    let app = TestApp::spawn().await;
    let res = app
        .anon()
        .post(app.url("/api/v1/does-not-exist"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        404,
        "CSRF passed; route simply does not exist"
    );
}

#[tokio::test]
async fn safe_methods_do_not_need_the_header() {
    let app = TestApp::spawn().await;
    let res = reqwest::Client::new()
        .get(app.url("/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    for method in [
        reqwest::Method::PUT,
        reqwest::Method::PATCH,
        reqwest::Method::DELETE,
    ] {
        let res = reqwest::Client::new()
            .request(method.clone(), app.url("/api/v1/me"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 403, "{method}");
    }
}

//! T050 — the built web app is served with an index.html fallback for client routes.
mod common;
use common::TestApp;

fn built() -> bool {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/dist/index.html")
        .exists()
}

#[tokio::test]
async fn client_routes_serve_index_html_when_the_web_app_is_built() {
    let app = TestApp::spawn().await;
    let res = app.anon().get(app.url("/dashboard")).send().await.unwrap();
    if built() {
        assert_eq!(res.status(), 200);
        assert!(res.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert_eq!(res.headers()["cache-control"], "no-cache");
        assert!(res.text().await.unwrap().contains("<div id=\"root\">"));
    } else {
        assert_eq!(res.status(), 404, "no web build embedded");
    }
}

#[tokio::test]
async fn unknown_api_paths_stay_json_404() {
    let app = TestApp::spawn().await;
    let res = app
        .anon()
        .get(app.url("/api/v1/nope"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "NOT_FOUND"
    );
}

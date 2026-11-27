//! T097 — `GET /users/search` for the notify picker.
mod common;

use common::factories::*;
use common::*;

fn names(out: &serde_json::Value) -> Vec<String> {
    out.as_array()
        .unwrap()
        .iter()
        .map(|u| u["email"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn search_matches_active_colleagues_in_the_same_org() {
    let app = TestApp::spawn().await;
    let (org, _admin, client) = org_with_admin(&app, "acme.test").await;
    create_user(&app, &org, NewUser::employee("ravi.kumar@acme.test")).await;
    create_user(
        &app,
        &org,
        NewUser::employee("ravina@acme.test").status("deactivated"),
    )
    .await;
    create_user(
        &app,
        &org,
        NewUser::employee("raveena@acme.test").status("invited"),
    )
    .await;
    let other = create_org(&app, "Other", "other.test").await;
    create_user(&app, &other, NewUser::employee("ravi@other.test")).await;

    let search = |q: &str| app.url(&format!("/api/v1/users/search?q={q}"));
    let (status, out) = get_json(&client, search("rav")).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(names(&out), ["ravi.kumar@acme.test"]);
    let first = &out[0];
    assert_eq!(first["full_name"], "ravi.kumar");
    assert!(first["id"].is_string());

    // Matches the name as well as the email, case-insensitively.
    let (_, out) = get_json(&client, search("RAVI.K")).await;
    assert_eq!(names(&out), ["ravi.kumar@acme.test"]);

    // Fewer than 2 characters → empty.
    let (status, out) = get_json(&client, search("r")).await;
    assert_eq!(status, 200);
    assert_eq!(out, serde_json::json!([]));
    let (_, out) = get_json(&client, search("%20%20")).await;
    assert_eq!(out, serde_json::json!([]));

    // `%` and `_` are literal, not wildcards.
    let (_, out) = get_json(&client, search("%25%25")).await;
    assert_eq!(out, serde_json::json!([]));

    let (status, _) = get_json(&app.anon(), search("rav")).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn at_most_ten_results() {
    let app = TestApp::spawn().await;
    let (org, _admin, client) = org_with_admin(&app, "acme.test").await;
    for i in 0..12 {
        create_user(
            &app,
            &org,
            NewUser::employee(&format!("dev{i:02}@acme.test")),
        )
        .await;
    }
    let (_, out) = get_json(&client, app.url("/api/v1/users/search?q=dev")).await;
    assert_eq!(names(&out).len(), 10);
    // Ordered by name.
    assert_eq!(names(&out)[0], "dev00@acme.test");
}

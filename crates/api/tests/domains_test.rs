//! T070 — whitelisted domains (FR-009).
mod common;

use common::factories::*;
use common::*;
use serde_json::json;

#[tokio::test]
async fn admin_lists_adds_and_removes_domains() {
    let app = TestApp::spawn().await;
    let (_org, _admin, admin) = org_with_admin(&app, "acme.test").await;
    let url = app.url("/api/v1/admin/org/domains");

    let (status, list) = get_json(&admin, url.clone()).await;
    assert_eq!(status, 200);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["domain"], "acme.test");

    let (status, added) = post_json(&admin, url.clone(), json!({ "domain": "  @Acme.IN " })).await;
    assert_eq!(status, 201, "{added}");
    assert_eq!(added["domain"], "acme.in");

    // New domain admits signups.
    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": "ravi@acme.in", "purpose": "signup", "full_name": "Ravi" }),
    )
    .await;
    assert_eq!(status, 202);

    let id = added["id"].as_str().unwrap();
    let (status, _) = delete(&admin, app.url(&format!("/api/v1/admin/org/domains/{id}"))).await;
    assert_eq!(status, 204);
    let (_, list) = get_json(&admin, url).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn refuses_invalid_public_and_taken_domains() {
    let app = TestApp::spawn().await;
    create_org(&app, "Other", "other.test").await;
    let (_org, _admin, admin) = org_with_admin(&app, "acme.test").await;
    let url = app.url("/api/v1/admin/org/domains");

    for (domain, status, code) in [
        ("not a domain", 422, "INVALID_DOMAIN"),
        ("gmail.com", 422, "PUBLIC_DOMAIN"),
        ("other.test", 409, "DOMAIN_TAKEN"),
        ("ACME.test", 409, "DOMAIN_TAKEN"),
    ] {
        let (s, body) = post_json(&admin, url.clone(), json!({ "domain": domain })).await;
        assert_eq!(
            (s, body["error"]["code"].as_str()),
            (status, Some(code)),
            "{domain}"
        );
    }
}

#[tokio::test]
async fn removed_domain_blocks_new_signups_but_existing_users_keep_access() {
    let app = TestApp::spawn().await;
    let (org, _admin, admin) = org_with_admin(&app, "acme.test").await;
    let (_, added) = post_json(
        &admin,
        app.url("/api/v1/admin/org/domains"),
        json!({ "domain": "acme.in" }),
    )
    .await;
    let existing = create_user(&app, &org, NewUser::employee("old@acme.in")).await;

    let id = added["id"].as_str().unwrap();
    let (status, _) = delete(&admin, app.url(&format!("/api/v1/admin/org/domains/{id}"))).await;
    assert_eq!(status, 204);

    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": "new@acme.in", "purpose": "signup", "full_name": "New" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("DOMAIN_NOT_REGISTERED"))
    );

    // The existing user can still sign in.
    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": existing.email, "purpose": "login" }),
    )
    .await;
    assert_eq!(status, 202);
    let code = codes_sent_to(&app, &existing.email)
        .await
        .pop()
        .expect("login code");
    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/verify"),
        json!({ "email": existing.email, "code": code, "purpose": "login" }),
    )
    .await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn only_admins_of_the_same_org_manage_domains() {
    let app = TestApp::spawn().await;
    let (org_a, _a, admin_a) = org_with_admin(&app, "acme.test").await;
    let (_org_b, _b, admin_b) = org_with_admin(&app, "beta.test").await;
    let employee = create_user(&app, &org_a, NewUser::employee("e@acme.test")).await;
    let emp = app.login_as(&employee).await;

    let (status, body) = get_json(&emp, app.url("/api/v1/admin/org/domains")).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (403, Some("FORBIDDEN"))
    );
    let (status, _) = post_json(
        &emp,
        app.url("/api/v1/admin/org/domains"),
        json!({ "domain": "acme.in" }),
    )
    .await;
    assert_eq!(status, 403);

    let (_, list) = get_json(&admin_a, app.url("/api/v1/admin/org/domains")).await;
    let id = list[0]["id"].as_str().unwrap();
    let (status, _) = delete(
        &admin_b,
        app.url(&format!("/api/v1/admin/org/domains/{id}")),
    )
    .await;
    assert_eq!(status, 404, "cross-org delete");
    let _ = org_a;
}

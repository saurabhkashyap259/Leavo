//! T095 — attachments (FR-029, FR-030): typed by content, 5 MB cap, 5-minute signed links
//! only for the owner, the request's eligible approvers and admins.
mod common;

use chrono::Duration;
use common::factories::*;
use common::*;
use reqwest::multipart::{Form, Part};
use rust_decimal_macros::dec;
use serde_json::json;

const MB5: usize = 5 * 1024 * 1024;

fn pdf(len: usize) -> Vec<u8> {
    let mut v = b"%PDF-1.4\n%%EOF\n".to_vec();
    v.resize(len.max(v.len()), b' ');
    v
}

fn samples() -> Vec<(&'static str, Vec<u8>, &'static str)> {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
    let jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1];
    let mut webp = b"RIFF\x24\0\0\0WEBPVP8 ".to_vec();
    webp.extend_from_slice(&[0; 16]);
    let mut heic = vec![0, 0, 0, 0x18];
    heic.extend_from_slice(b"ftypheic\0\0\0\0mif1heic");
    heic.extend_from_slice(&[0; 8]);
    vec![
        ("note.pdf", pdf(100), "application/pdf"),
        ("scan.png", png, "image/png"),
        ("photo.jpg", jpeg, "image/jpeg"),
        ("photo.webp", webp, "image/webp"),
        ("IMG_0001.HEIC", heic, "image/heif"),
    ]
}

async fn upload(
    app: &TestApp,
    client: &reqwest::Client,
    name: &str,
    bytes: Vec<u8>,
) -> (u16, serde_json::Value) {
    let form = Form::new().part("file", Part::bytes(bytes).file_name(name.to_string()));
    let res = client
        .post(app.url("/api/v1/attachments"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or(serde_json::Value::Null))
}

/// Path and query of an absolute URL, to call it against the test server.
fn local_path(url: &str) -> String {
    let rest = url.split_once("://").unwrap().1;
    rest[rest.find('/').unwrap()..].to_string()
}

#[tokio::test]
async fn accepts_pdf_and_images_by_content() {
    let app = TestApp::spawn().await;
    let (org, _admin, _) = org_with_admin(&app, "acme.test").await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let client = app.login_as(&asha).await;
    for (name, bytes, ct) in samples() {
        let size = bytes.len();
        let (status, out) = upload(&app, &client, name, bytes).await;
        assert_eq!(status, 201, "{name}: {out}");
        assert_eq!(out["name"], name);
        assert_eq!(out["content_type"], ct, "{name}");
        assert_eq!(out["size"], size);
    }
}

#[tokio::test]
async fn rejects_large_and_disguised_files() {
    let app = TestApp::spawn().await;
    let (org, _admin, _) = org_with_admin(&app, "acme.test").await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let client = app.login_as(&asha).await;

    let (status, out) = upload(&app, &client, "exact.pdf", pdf(MB5)).await;
    assert_eq!(status, 201, "{out}");
    let (status, out) = upload(&app, &client, "big.pdf", pdf(MB5 + 1)).await;
    assert_eq!(status, 413, "{out}");
    assert_eq!(out["error"]["code"], "FILE_TOO_LARGE");

    let mut exe = b"MZ".to_vec();
    exe.extend_from_slice(&[0x90, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0xFF, 0xFF]);
    let (status, out) = upload(&app, &client, "invoice.pdf", exe).await;
    assert_eq!(status, 415, "{out}");
    assert_eq!(out["error"]["code"], "FILE_TYPE_NOT_ALLOWED");

    // Nothing stored for refused files.
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM attachments")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let (status, _) = upload(&app, &app.anon(), "note.pdf", pdf(10)).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn signed_links_last_five_minutes_and_follow_leave_visibility() {
    let app = TestApp::spawn().await;
    let (org, admin, admin_client) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let priya = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let vik = create_user(
        &app,
        &org,
        NewUser::employee("vik@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya]).await;
    create_group(&app, &org, "Sales", &[&vik]).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    let client = app.login_as(&asha).await;

    let (_, up) = upload(&app, &client, "note.pdf", pdf(100)).await;
    let id = up["id"].as_str().unwrap().to_string();
    let url_of = |c: reqwest::Client| {
        let u = app.url(&format!("/api/v1/attachments/{id}/url"));
        async move { get_json(&c, u).await }
    };

    // Before it is attached to a request, only the owner can open it.
    let (status, link) = url_of(client.clone()).await;
    assert_eq!(status, 200, "{link}");
    assert_eq!(link["expires_at"], "2026-11-24T09:05:00Z");
    assert!(link["url"]
        .as_str()
        .unwrap()
        .starts_with("http://leavo.test/"));
    assert_eq!(url_of(app.login_as(&priya).await).await.0, 404);

    // Attach it to a request.
    let mut body = leave_body(cl, "2026-12-01", "2026-12-01");
    body["attachment_id"] = json!(id);
    let (status, out) = post_leave(&app, &client, &body, &key()).await;
    assert_eq!(status, 201, "{out}");
    let (_, detail) = get_json(
        &client,
        app.url(&format!("/api/v1/leaves/{}", out["id"].as_str().unwrap())),
    )
    .await;
    assert_eq!(detail["attachment"]["id"], id.as_str());
    assert_eq!(detail["attachment"]["name"], "note.pdf");

    assert_eq!(url_of(app.login_as(&priya).await).await.0, 200);
    assert_eq!(url_of(admin_client.clone()).await.0, 200);
    assert_eq!(url_of(app.login_as(&vik).await).await.0, 404);
    assert_eq!(url_of(app.login_as(&ravi).await).await.0, 404);
    let _ = &admin;

    // The link downloads the file without a session…
    let (_, link) = url_of(client.clone()).await;
    let download = app.url(&local_path(link["url"].as_str().unwrap()));
    let res = app.anon().get(&download).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "application/pdf");
    assert_eq!(res.bytes().await.unwrap().to_vec(), pdf(100));

    // …until it expires, and a tampered link never works.
    let tampered = download.replace("sig=", "sig=0");
    assert_eq!(
        app.anon().get(&tampered).send().await.unwrap().status(),
        403
    );
    app.clock.advance(Duration::seconds(301));
    assert_eq!(
        app.anon().get(&download).send().await.unwrap().status(),
        403
    );
}

#[tokio::test]
async fn someone_elses_attachment_cannot_be_used() {
    let app = TestApp::spawn().await;
    let (org, _admin, _) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    add_ledger_row(&app, &ravi, cl, 2026, dec!(12), "allotment").await;
    let (_, up) = upload(&app, &app.login_as(&asha).await, "note.pdf", pdf(100)).await;

    let mut body = leave_body(cl, "2026-12-01", "2026-12-01");
    body["attachment_id"] = up["id"].clone();
    let (status, out) = post_leave(&app, &app.login_as(&ravi).await, &body, &key()).await;
    assert_eq!(status, 422, "{out}");
    assert_eq!(out["error"]["details"]["field"], "attachment_id");
}

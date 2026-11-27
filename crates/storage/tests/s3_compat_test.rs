//! T096 — S3-compatible storage (FR-053) against a SeaweedFS container: the same behaviour as
//! local storage, plus a pre-signed download URL that expires after 5 minutes.
//!
//! MinIO no longer publishes container images, so the test uses SeaweedFS's S3 gateway with
//! signature checking switched on (an identity file), which is what makes the tamper check real.
mod common;

use common::*;
use leavo_storage::{S3Settings, Storage, SIGNED_URL_TTL};
use std::time::Duration;
use testcontainers::core::{ExecCommand, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const IDENTITIES: &str = r#"{"identities":[{"name":"leavo","credentials":[{"accessKey":"leavo-key","secretKey":"leavo-secret"}],"actions":["Admin","Read","Write","List","Tagging"]}]}"#;

#[tokio::test]
async fn s3_backend_matches_local_behaviour() {
    let s3 = GenericImage::new("chrislusf/seaweedfs", "4.51")
        .with_exposed_port(8333.tcp())
        .with_wait_for(WaitFor::seconds(1))
        .with_copy_to("/etc/leavo-s3.json", IDENTITIES.as_bytes().to_vec())
        .with_cmd([
            "server",
            "-dir=/data",
            "-s3",
            "-s3.config=/etc/leavo-s3.json",
        ])
        .start()
        .await
        .expect("start SeaweedFS (is Docker running?)");

    // The master needs a moment before `weed shell` can create the bucket.
    let mut created = false;
    for _ in 0..60 {
        let mut out = s3
            .exec(ExecCommand::new([
                "sh",
                "-c",
                "echo 's3.bucket.create -name leavo' | weed shell 2>&1",
            ]))
            .await
            .expect("exec weed shell");
        let text = String::from_utf8_lossy(&out.stdout_to_vec().await.unwrap()).to_string();
        if out.exit_code().await.unwrap() == Some(0) && !text.contains("error") {
            created = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(created, "could not create the bucket");

    let port = s3.get_host_port_ipv4(8333.tcp()).await.unwrap();
    let host = s3.get_host().await.unwrap();
    let endpoint = format!("http://{host}:{port}");
    for _ in 0..60 {
        if reqwest::get(&endpoint).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let store = Storage::s3(&S3Settings {
        endpoint,
        region: "us-east-1".into(),
        bucket: "leavo".into(),
        access_key: "leavo-key".into(),
        secret_key: "leavo-secret".into(),
    })
    .unwrap();

    check_backend(&store, "org/user").await;

    let url = store
        .presigned_get("org/user/pdf")
        .await
        .unwrap()
        .expect("S3 gives a pre-signed URL");
    assert_eq!(SIGNED_URL_TTL.as_secs(), 300);
    assert!(
        url.contains("X-Amz-Expires=300"),
        "URL should expire in 5 minutes: {url}"
    );
    let res = reqwest::get(&url).await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.bytes().await.unwrap().to_vec(), pdf());

    // A tampered signature is refused.
    let bad = url.replace("X-Amz-Signature=", "X-Amz-Signature=0");
    assert_eq!(reqwest::get(&bad).await.unwrap().status(), 403);
}

//! T049 — the committed OpenAPI document matches the code (drift check, FR-050).
use leavo_api::openapi::openapi_yaml;

const COMMITTED: &str = "../../specs/001-leave-management-v1/contracts/openapi.yaml";

#[test]
fn generated_document_describes_the_api() {
    let yaml = openapi_yaml();
    assert!(yaml.contains("openapi: 3.1"));
    assert!(yaml.contains("/api/v1/me"));
    assert!(yaml.contains("Leavo API"));
}

#[test]
fn committed_document_is_up_to_date() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED);
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        committed,
        openapi_yaml(),
        "contracts/openapi.yaml is stale; run `cargo run -p leavo-api -- openapi --write`"
    );
}

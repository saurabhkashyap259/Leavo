//! T051 — CLI surface (contracts/configuration.md).
use std::process::Command;

fn leavo(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_leavo"))
        .args(args)
        .env_clear()
        .output()
        .unwrap()
}

#[test]
fn version_prints_package_version() {
    let out = leavo(&["--version"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_every_command() {
    let out = leavo(&["--help"]);
    let help = String::from_utf8_lossy(&out.stdout);
    for cmd in [
        "serve",
        "worker",
        "migrate",
        "openapi",
        "setup-token",
        "recovery-login",
    ] {
        assert!(help.contains(cmd), "missing {cmd} in:\n{help}");
    }
}

#[test]
fn openapi_prints_yaml_without_config() {
    let out = leavo(&["openapi"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("openapi:"));
}

#[test]
fn serve_fails_fast_on_missing_config() {
    let out = leavo(&["serve"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("DATABASE_URL"));
}

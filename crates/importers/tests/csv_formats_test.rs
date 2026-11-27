//! T192 — canonical import CSVs (contracts/csv-formats.md §2, §3, §5): parsing and the dry-run
//! plan against the organization, with every error and warning code.
use chrono::NaiveDate;
use leavo_importers::csv::{parse_balances, parse_history, plan, ExistingUser, Issue, OrgContext};
use rust_decimal_macros::dec;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

/// Acme: domain acme.test, types CL and SL, group Eng. Asha and Ravi exist; Ravi already had an
/// imported CL balance for 2026 and is booked on 24 Nov 2026. The current leave year is 2026.
fn ctx() -> OrgContext {
    OrgContext {
        domains: BTreeSet::from(["acme.test".to_string()]),
        leave_types: BTreeMap::from([
            ("CL".to_string(), Uuid::from_u128(1)),
            ("SL".to_string(), Uuid::from_u128(2)),
        ]),
        users: BTreeMap::from([
            (
                "asha@acme.test".to_string(),
                ExistingUser {
                    id: Uuid::from_u128(10),
                    imported: BTreeSet::new(),
                    booked: BTreeSet::new(),
                },
            ),
            (
                "ravi@acme.test".to_string(),
                ExistingUser {
                    id: Uuid::from_u128(11),
                    imported: BTreeSet::from([("CL".to_string(), 2026)]),
                    booked: BTreeSet::from([d("2026-11-24")]),
                },
            ),
            (
                "priya@acme.test".to_string(),
                ExistingUser {
                    id: Uuid::from_u128(12),
                    imported: BTreeSet::new(),
                    booked: BTreeSet::new(),
                },
            ),
        ]),
        groups: BTreeSet::from(["eng".to_string()]),
        current_leave_year: 2026,
        start_month: 1,
    }
}

fn codes(issues: &[Issue]) -> Vec<(&str, usize, &str)> {
    issues.iter().map(|i| (i.file, i.row, i.code)).collect()
}

#[test]
fn a_clean_file_becomes_a_plan_with_warnings() {
    let (balances, mut issues) = parse_balances(&fixture("balances_ok.csv"));
    let (history, more) = parse_history(&fixture("history_ok.csv"));
    issues.extend(more);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(balances.len(), 4);
    assert_eq!(balances[2].leave_type_code, "CL", "codes are upper-cased");
    assert_eq!(balances[2].available_days, dec!(-1));
    assert_eq!(balances[0].approver_group.as_deref(), Some("Eng"));
    assert_eq!(history[1].days, dec!(0.5));
    assert_eq!(history[0].reason, "Trip");
    assert_eq!(history[1].reason, "Imported from Keka", "default reason");

    let p = plan(balances, history, issues, &ctx());
    assert_eq!(p.errors, []);
    assert_eq!(
        codes(&p.warnings),
        [
            ("balances", 2, "USER_EXISTS"),
            ("balances", 4, "USER_EXISTS"),
            ("balances", 4, "BALANCE_REPLACES_EARLIER_IMPORT"),
            ("balances", 5, "GROUP_WILL_BE_CREATED"),
            ("history", 4, "HISTORY_OUTSIDE_LEAVE_YEAR"),
        ]
    );
    assert_eq!(p.users_to_create.len(), 1);
    assert_eq!(p.users_to_create[0].email, "new@acme.test");
    assert_eq!(p.users_to_create[0].approver_group.as_deref(), Some("Ops"));
    assert_eq!(p.groups_to_create, ["Ops"]);
    assert_eq!(p.balances.len(), 4);
    assert_eq!(p.history.len(), 3);
}

#[test]
fn every_balance_error_is_reported_by_row() {
    let (balances, issues) = parse_balances(&fixture("balances_errors.csv"));
    let p = plan(balances, vec![], issues, &ctx());
    assert_eq!(
        codes(&p.errors),
        [
            ("balances", 2, "UNKNOWN_DOMAIN"),
            ("balances", 3, "UNKNOWN_LEAVE_TYPE"),
            ("balances", 4, "INVALID_DATE"),
            ("balances", 5, "INVALID_DAYS"),
            ("balances", 7, "DUPLICATE_ROW"),
            ("balances", 8, "MISSING_VALUE"),
        ]
    );
    // Rows with errors are not part of the plan.
    assert_eq!(p.balances.len(), 1);
}

#[test]
fn every_history_error_is_reported_by_row() {
    let (history, issues) = parse_history(&fixture("history_errors.csv"));
    let p = plan(vec![], history, issues, &ctx());
    assert_eq!(
        codes(&p.errors),
        [
            ("history", 2, "INVALID_DATE"),
            ("history", 3, "INVALID_DATE"),
            ("history", 4, "INVALID_DAYS"),
            ("history", 6, "OVERLAP"),
            ("history", 7, "OVERLAP"),
            ("history", 8, "UNKNOWN_USER"),
        ]
    );
}

#[test]
fn a_missing_column_stops_the_file() {
    let (rows, issues) = parse_balances(&fixture("missing_column.csv"));
    assert!(rows.is_empty());
    assert_eq!(codes(&issues), [("balances", 1, "MISSING_COLUMN")]);
    assert!(
        issues[0].message.contains("leave_year"),
        "{}",
        issues[0].message
    );
    let (rows, issues) = parse_history(b"email,from_date\n");
    assert!(rows.is_empty());
    assert_eq!(issues[0].code, "MISSING_COLUMN");
}

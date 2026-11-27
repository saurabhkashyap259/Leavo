//! T193 — Keka exports (contracts/csv-formats.md §4): raw columns, found by any of their usual
//! header names, mapped to canonical rows; Keka leave type names mapped to Leavo codes by a
//! configurable map. Only approved history is imported.
use leavo_importers::keka::{keka_leave_types, map_keka, KekaMap};
use rust_decimal_macros::dec;

const BALANCES: &str =
    "\u{feff}Employee Number,Employee Name,Email,Date Of Joining,Leave Type,Available Balance\n\
E-1,Asha Rao,Asha@Acme.test,01-Apr-2025,Casual Leave,8\n\
E-1,Asha Rao,asha@acme.test,01-Apr-2025,Sick Leave,2.5\n\
E-2,Ravi K,ravi@acme.test,,Optional Holiday,1\n";

const HISTORY: &str = "Employee Name,Work Email,Leave Type,From Date,To Date,No Of Days,Status,Reason,Approved By Email\n\
Asha Rao,asha@acme.test,Casual Leave,02/11/2026,03/11/2026,2,Approved,Trip,priya@acme.test\n\
Asha Rao,asha@acme.test,Casual Leave,10-Nov-2026,10-Nov-2026,1,Rejected,No,\n\
Ravi K,ravi@acme.test,Sick Leave,05 Oct 2026,05 Oct 2026,0.5,approved,,\n";

fn map() -> KekaMap {
    KekaMap::from([
        ("casual leave".to_string(), "CL".to_string()),
        ("sick leave".to_string(), "SL".to_string()),
    ])
}

#[test]
fn lists_the_leave_type_names_to_map() {
    assert_eq!(
        keka_leave_types(BALANCES.as_bytes(), Some(HISTORY.as_bytes())),
        ["Casual Leave", "Optional Holiday", "Sick Leave"]
    );
}

#[test]
fn maps_balances_to_canonical_rows() {
    let out = map_keka(BALANCES.as_bytes(), None, &map(), 2026);
    let asha = &out.balances[0];
    assert_eq!(asha.row, 2);
    assert_eq!(asha.email, "asha@acme.test");
    assert_eq!(asha.full_name.as_deref(), Some("Asha Rao"));
    assert_eq!(asha.employee_code.as_deref(), Some("E-1"));
    assert_eq!(asha.date_of_joining, "2025-04-01".parse().ok());
    assert_eq!(asha.leave_type_code, "CL");
    assert_eq!(asha.leave_year, 2026);
    assert_eq!(asha.available_days, dec!(8));
    assert_eq!(out.balances[1].leave_type_code, "SL");
    assert_eq!(out.balances[1].available_days, dec!(2.5));
    // An unmapped Keka type is an error naming it.
    assert_eq!(out.balances.len(), 2);
    assert_eq!(out.issues.len(), 1);
    assert_eq!(
        (out.issues[0].row, out.issues[0].code),
        (4, "UNKNOWN_LEAVE_TYPE")
    );
    assert!(out.issues[0].message.contains("Optional Holiday"));
}

#[test]
fn maps_approved_history_only() {
    let out = map_keka(BALANCES.as_bytes(), Some(HISTORY.as_bytes()), &map(), 2026);
    assert_eq!(out.history.len(), 2);
    let trip = &out.history[0];
    assert_eq!(
        (trip.from_date.to_string(), trip.to_date.to_string()),
        ("2026-11-02".into(), "2026-11-03".into())
    );
    assert_eq!(trip.days, dec!(2));
    assert_eq!(trip.reason, "Trip");
    assert_eq!(trip.approved_by_email.as_deref(), Some("priya@acme.test"));
    let sick = &out.history[1];
    assert_eq!(sick.row, 4);
    assert_eq!(sick.leave_type_code, "SL");
    assert_eq!(sick.days, dec!(0.5));
    assert_eq!(out.skipped_history, 1);
}

#[test]
fn missing_keka_columns_are_named() {
    let out = map_keka(b"Name,Leave Type\nA,Casual Leave\n", None, &map(), 2026);
    assert_eq!(out.issues[0].code, "MISSING_COLUMN");
    assert!(
        out.issues[0].message.contains("Email"),
        "{}",
        out.issues[0].message
    );
}

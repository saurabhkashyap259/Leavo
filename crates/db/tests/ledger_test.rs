//! T019 — append-only ledger and ledger-only balance view (FR-040, FR-032, constitution V).
use leavo_db::ledger::{insert_entry, ledger_balances, LedgerEntry, LedgerReason};
use leavo_db::testkit::TestDb;
use leavo_db::tx::begin_org_tx;
use rust_decimal_macros::dec;
use uuid::Uuid;

struct Fixture {
    db: TestDb,
    org: Uuid,
    user: Uuid,
    cl: Uuid,
    co: Uuid,
}

async fn fixture() -> Fixture {
    let db = TestDb::new().await;
    let (org, user, cl, co) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let o = &db.owner;
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'Acme')")
        .bind(org)
        .execute(o)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO users (id, org_id, email, full_name) VALUES ($1, $2, 'a@acme.test', 'A')",
    )
    .bind(user)
    .bind(org)
    .execute(o)
    .await
    .unwrap();
    sqlx::query("INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota, receives_carry_forward) VALUES ($1, $2, 'CL', 'Casual', '#3B82F6', 12, true)")
        .bind(cl).bind(org).execute(o).await.unwrap();
    sqlx::query("INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota, is_comp_off) VALUES ($1, $2, 'CO', 'Comp-off', '#10B981', NULL, true)")
        .bind(co).bind(org).execute(o).await.unwrap();
    Fixture {
        db,
        org,
        user,
        cl,
        co,
    }
}

fn entry(
    f: &Fixture,
    ty: Uuid,
    year: i32,
    delta: rust_decimal::Decimal,
    reason: LedgerReason,
) -> LedgerEntry {
    LedgerEntry {
        org_id: f.org,
        user_id: f.user,
        leave_type_id: ty,
        leave_year: year,
        delta,
        reason,
        ref_kind: None,
        ref_id: None,
        note: if reason == LedgerReason::ManualAdjustment {
            Some("fix".into())
        } else {
            None
        },
        created_by: None,
    }
}

#[tokio::test]
async fn ledger_sums_per_category_and_year() {
    let f = fixture().await;
    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    use LedgerReason::*;
    for (year, delta, reason) in [
        (2026, dec!(12), Allotment),
        (2026, dec!(-3), LeaveTaken),
        (2026, dec!(1), LeaveReversed),
        (2026, dec!(0.5), ManualAdjustment),
        (2027, dec!(12), Allotment),
        (2027, dec!(5), CarryForward),
    ] {
        insert_entry(&mut tx, &entry(&f, f.cl, year, delta, reason))
            .await
            .unwrap();
    }
    let rows = ledger_balances(&mut tx, f.user).await.unwrap();
    let y26 = rows
        .iter()
        .find(|r| r.leave_type_id == f.cl && r.leave_year == Some(2026))
        .unwrap();
    assert_eq!(y26.allotted, dec!(12));
    assert_eq!(y26.taken, dec!(2));
    assert_eq!(y26.adjusted, dec!(0.5));
    assert_eq!(y26.carried, dec!(0));
    assert_eq!(y26.balance, dec!(10.5));
    let y27 = rows
        .iter()
        .find(|r| r.leave_type_id == f.cl && r.leave_year == Some(2027))
        .unwrap();
    assert_eq!(y27.carried, dec!(5));
    assert_eq!(y27.balance, dec!(17));
}

#[tokio::test]
async fn comp_off_balance_ignores_leave_year() {
    let f = fixture().await;
    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    insert_entry(
        &mut tx,
        &entry(&f, f.co, 2026, dec!(1), LedgerReason::CompOffCredit),
    )
    .await
    .unwrap();
    insert_entry(
        &mut tx,
        &entry(&f, f.co, 2027, dec!(0.5), LedgerReason::CompOffCredit),
    )
    .await
    .unwrap();
    insert_entry(
        &mut tx,
        &entry(&f, f.co, 2027, dec!(-1), LedgerReason::LeaveTaken),
    )
    .await
    .unwrap();
    let rows = ledger_balances(&mut tx, f.user).await.unwrap();
    let co: Vec<_> = rows.iter().filter(|r| r.leave_type_id == f.co).collect();
    assert_eq!(co.len(), 1, "one comp-off balance across years");
    assert_eq!(co[0].leave_year, None);
    assert_eq!(co[0].balance, dec!(0.5));
}

#[tokio::test]
async fn ledger_rows_cannot_be_updated_or_deleted_by_the_app() {
    let f = fixture().await;
    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    insert_entry(
        &mut tx,
        &entry(&f, f.cl, 2026, dec!(12), LedgerReason::Allotment),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    let upd = sqlx::query("UPDATE balance_ledger SET delta = 99")
        .execute(&mut *tx)
        .await;
    assert!(upd.unwrap_err().to_string().contains("permission denied"));
    tx.rollback().await.unwrap();

    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    let del = sqlx::query("DELETE FROM balance_ledger")
        .execute(&mut *tx)
        .await;
    assert!(del.unwrap_err().to_string().contains("permission denied"));
}

#[tokio::test]
async fn invalid_entries_are_rejected() {
    let f = fixture().await;
    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    let mut no_note = entry(&f, f.cl, 2026, dec!(1), LedgerReason::ManualAdjustment);
    no_note.note = None;
    assert!(
        insert_entry(&mut tx, &no_note).await.is_err(),
        "manual adjustment needs a note"
    );
    tx.rollback().await.unwrap();

    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    assert!(
        insert_entry(
            &mut tx,
            &entry(&f, f.cl, 2026, dec!(0.3), LedgerReason::Allotment)
        )
        .await
        .is_err(),
        "half-day multiples only"
    );
    tx.rollback().await.unwrap();

    let mut tx = begin_org_tx(&f.db.app, f.org).await.unwrap();
    assert!(
        insert_entry(
            &mut tx,
            &entry(&f, f.cl, 2026, dec!(0), LedgerReason::Allotment)
        )
        .await
        .is_err(),
        "zero deltas are meaningless"
    );
}

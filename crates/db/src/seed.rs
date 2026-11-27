//! Default leave types for a new organization (T080, FR-011, data-model.md §leave_types).
//! Every behaviour comes from these settings — nothing is keyed on a code or name.
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

struct Seed {
    code: &'static str,
    name: &'static str,
    color: &'static str,
    quota: Option<i64>,
    restricted: bool,
    carry: bool,
    backdate: i32,
    attachment_above: Option<i64>,
}

const DEFAULTS: &[Seed] = &[
    Seed {
        code: "CL",
        name: "Casual Leave",
        color: "#3B82F6",
        quota: Some(12),
        restricted: false,
        carry: true,
        backdate: 0,
        attachment_above: None,
    },
    Seed {
        code: "SL",
        name: "Sick Leave",
        color: "#EF4444",
        quota: Some(6),
        restricted: false,
        carry: false,
        backdate: 7,
        attachment_above: Some(2),
    },
    Seed {
        code: "CCL",
        name: "Child Care Leave",
        color: "#F59E0B",
        quota: Some(10),
        restricted: true,
        carry: false,
        backdate: 0,
        attachment_above: None,
    },
    Seed {
        code: "ML",
        name: "Maternity Leave",
        color: "#EC4899",
        quota: Some(130),
        restricted: true,
        carry: false,
        backdate: 0,
        attachment_above: None,
    },
    Seed {
        code: "PL",
        name: "Paternity Leave",
        color: "#8B5CF6",
        quota: Some(5),
        restricted: true,
        carry: false,
        backdate: 0,
        attachment_above: None,
    },
    Seed {
        code: "CO",
        name: "Comp-off",
        color: "#10B981",
        quota: None,
        restricted: false,
        carry: false,
        backdate: 0,
        attachment_above: None,
    },
];

/// Insert the default types into `org`. Runs inside an org-scoped transaction.
pub async fn seed_default_leave_types(conn: &mut PgConnection, org: Uuid) -> sqlx::Result<()> {
    for (i, s) in DEFAULTS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO leave_types
                 (id, org_id, code, name, color, yearly_quota, is_comp_off, receives_carry_forward,
                  max_backdate_days, attachment_required_above_days, availability, sort_order)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::leave_availability, $12)",
        )
        .bind(Uuid::now_v7())
        .bind(org)
        .bind(s.code)
        .bind(s.name)
        .bind(s.color)
        .bind(s.quota.map(Decimal::from))
        .bind(s.quota.is_none())
        .bind(s.carry)
        .bind(s.backdate)
        .bind(s.attachment_above.map(Decimal::from))
        .bind(if s.restricted { "restricted" } else { "all" })
        .bind(i as i32 + 1)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

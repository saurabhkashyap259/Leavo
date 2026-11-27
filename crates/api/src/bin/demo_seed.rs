//! `leavo-demo-seed` — fill an instance with a demo organization (T220, FR-066).
//!
//! ```text
//! DATABASE_URL=… leavo-demo-seed            # refuses when an organization already exists
//! DATABASE_URL=… leavo-demo-seed --reset    # deletes every organization first (nightly reset)
//! DATABASE_URL=… leavo-demo-seed --employees 1000   # adds 1,000 generated employees (load tests)
//! ```
//!
//! Creates "Demo Co" on `DEMO_DOMAIN` (default `demo.leavo.test`) with the default leave
//! types, this year's India holidays, approver groups, an admin, approvers and employees with
//! allotments, and a mix of approved and pending leave. Setup is marked complete. People sign in
//! with codes from the demo mailbox (see deploy/demo/README.md).
use chrono::{Datelike, Days, Utc, Weekday};
use leavo_api::bootstrap::{self, NewOrgWithAdmin};
use leavo_core::calendar::{working_days, Half, WeekendMask};
use leavo_db::leaves_repo::{self, NewLeaveRequest};
use leavo_db::users_repo::{self, NewUserRow};
use leavo_db::{approvals_repo, tx};
use std::process::ExitCode;
use uuid::Uuid;

const PEOPLE: &[(&str, &str, &str, &str)] = &[
    // (local part, name, role, group)
    ("priya", "Priya Shah", "approver", "Engineering"),
    ("vik", "Vikram Rao", "approver", "Sales"),
    ("asha", "Asha Rao", "employee", "Engineering"),
    ("ravi", "Ravi Kumar", "employee", "Engineering"),
    ("meena", "Meena Iyer", "employee", "Engineering"),
    ("arjun", "Arjun Mehta", "employee", "Engineering"),
    ("sara", "Sara Thomas", "employee", "Sales"),
    ("kabir", "Kabir Singh", "employee", "Sales"),
    ("nisha", "Nisha Gupta", "employee", "Sales"),
    ("dev", "Dev Patel", "employee", "Operations"),
];

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reset = args.iter().any(|a| a == "--reset");
    let extra: usize = match args.iter().position(|a| a == "--employees") {
        Some(i) => args
            .get(i + 1)
            .and_then(|n| n.parse().ok())
            .ok_or("--employees needs a whole number, e.g. --employees 1000")?,
        None => 0,
    };
    let url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required")?;
    let domain = std::env::var("DEMO_DOMAIN").unwrap_or_else(|_| "demo.leavo.test".into());
    let owner = leavo_db::pool::connect_owner(&url, 2).await?;
    leavo_db::migrate::run(&owner).await?;
    let mut conn = owner.begin().await?;
    let orgs: i64 = sqlx::query_scalar("SELECT count(*) FROM organizations")
        .fetch_one(&mut *conn)
        .await?;
    if orgs > 0 && !reset {
        return Err("organizations already exist; pass --reset to replace them".into());
    }
    if reset {
        // Jobs and codes are not org-owned rows; clear them with the organizations.
        sqlx::query("DELETE FROM jobs").execute(&mut *conn).await?;
        sqlx::query("DELETE FROM login_codes")
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM rate_limits")
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM organizations")
            .execute(&mut *conn)
            .await?;
    }
    let now = Utc::now();
    let year = now.year();
    let (org, admin) = bootstrap::create(
        &mut conn,
        &NewOrgWithAdmin {
            name: "Demo Co",
            timezone: "Asia/Kolkata",
            weekend_mask: 96,
            leave_year_start_month: 1,
            admin_email: &format!("admin@{domain}"),
            admin_name: "Ada Admin",
            domains: std::slice::from_ref(&domain),
            holiday_pack: Some(&format!("IN/{year}")),
            seed_leave_types: true,
            audit_action: "org.setup",
            ip: None,
        },
        now,
    )
    .await
    .map_err(|e| format!("{e:?}"))?;
    tx::set_org(&mut conn, org).await?;
    sqlx::query("UPDATE organizations SET carry_forward_cap = 5 WHERE id = $1")
        .bind(org)
        .execute(&mut *conn)
        .await?;
    users_repo::allot_on_join(&mut conn, org, admin, None, now).await?;

    let mut groups = std::collections::BTreeMap::new();
    for g in ["Engineering", "Sales", "Operations"] {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(org)
            .bind(g)
            .execute(&mut *conn)
            .await?;
        groups.insert(g, id);
    }
    // The named demo people, then any generated employees spread over the three groups.
    let group_names = ["Engineering", "Sales", "Operations"];
    let people = PEOPLE
        .iter()
        .map(|&(local, name, role, group)| (local.to_string(), name.to_string(), role, group))
        .chain((1..=extra).map(|n| {
            (
                format!("person{n}"),
                format!("Person {n}"),
                "employee",
                group_names[n % group_names.len()],
            )
        }));
    let mut ids = Vec::new();
    for (local, name, role, group) in people {
        let id = users_repo::insert_user(
            &mut conn,
            &NewUserRow {
                org_id: org,
                email: &format!("{local}@{domain}"),
                full_name: &name,
                role,
                status: "active",
                date_of_joining: None,
            },
            now,
        )
        .await?;
        let group_id = groups[group];
        if role == "approver" {
            sqlx::query("INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)")
                .bind(group_id)
                .bind(id)
                .bind(org)
                .execute(&mut *conn)
                .await?;
        } else {
            sqlx::query("UPDATE users SET approver_group_id = $2, department = $3 WHERE id = $1")
                .bind(id)
                .bind(group_id)
                .bind(group)
                .execute(&mut *conn)
                .await?;
        }
        users_repo::allot_on_join(&mut conn, org, id, None, now).await?;
        ids.push((id, role, groups[group]));
    }
    // Operations has no approver of its own: its requests go to every approver.
    let cl: Uuid = sqlx::query_scalar("SELECT id FROM leave_types WHERE code = 'CL'")
        .fetch_one(&mut *conn)
        .await?;
    let cal = leaves_repo::org_calendar(&mut conn, org, now).await?;
    // A spread of leave around today: some approved (so "who's away" has people), some pending.
    let today = cal.today;
    let employees: Vec<_> = ids.iter().filter(|p| p.1 == "employee").collect();
    for (i, (user, _, group)) in employees.iter().enumerate() {
        let offset = (i as u64 * 3) % 20;
        let mut from = today.checked_add_days(Days::new(offset)).unwrap_or(today);
        while matches!(from.weekday(), Weekday::Sat | Weekday::Sun) {
            from = from.succ_opt().unwrap_or(from);
        }
        let to = from
            .checked_add_days(Days::new(i as u64 % 2))
            .unwrap_or(from);
        let holidays = leaves_repo::holidays_between(&mut conn, from, to).await?;
        let count = working_days(
            from,
            to,
            None,
            None,
            WeekendMask::from_bits(96).unwrap_or_default(),
            &holidays,
            1,
        );
        if count.days.is_empty() {
            continue;
        }
        let id = Uuid::now_v7();
        leaves_repo::insert_request(
            &mut conn,
            &NewLeaveRequest {
                id,
                org_id: org,
                user_id: *user,
                leave_type_id: cl,
                from,
                to,
                start_half: None::<Half>,
                end_half: None,
                days: count.total(),
                reason: "Family function",
                routed_group_id: Some(*group),
                now,
            },
            &count.days,
        )
        .await?;
        if i % 3 != 2 {
            sqlx::query(
                "UPDATE leave_requests SET status = 'approved', decided_by = $2, decided_at = $3,
                        version = version + 1 WHERE id = $1",
            )
            .bind(id)
            .bind(admin)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let r = approvals_repo::load(&mut conn, id)
                .await?
                .ok_or("request vanished")?;
            approvals_repo::charge(&mut conn, &r, admin).await?;
        }
    }
    sqlx::query(
        "UPDATE instance_settings SET setup_completed_at = $1, setup_token_hash = NULL WHERE id = 1",
    )
    .bind(now)
    .execute(&mut *conn)
    .await?;
    conn.commit().await?;
    println!(
        "Demo Co is ready: sign in as admin@{domain} (or priya@, asha@ …{domain}) with a code from the demo mailbox."
    );
    Ok(())
}

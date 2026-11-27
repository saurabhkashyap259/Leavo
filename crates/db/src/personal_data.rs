//! Personal data (T188, FR-045): export everything about a person, and anonymise a deactivated
//! person while keeping dates, day counts and balance entries. Runs in org-scoped transactions.
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

/// Profile, leave and comp-off history and balance entries as one JSON document.
pub async fn export(
    conn: &mut PgConnection,
    user: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<Value> {
    let profile: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'id', u.id, 'email', u.email, 'full_name', u.full_name, 'role', u.role,
            'status', u.status, 'employee_code', u.employee_code, 'department', u.department,
            'date_of_joining', u.date_of_joining, 'approver_group', g.name,
            'organization', o.name, 'created_at', u.created_at)
         FROM users u JOIN organizations o ON o.id = u.org_id
         LEFT JOIN approver_groups g ON g.id = u.approver_group_id WHERE u.id = $1",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    let leave: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', r.id, 'leave_type', t.code, 'from', r.from_date, 'to', r.to_date,
            'start_half', r.start_half, 'end_half', r.end_half, 'days', r.days,
            'status', r.status, 'reason', r.reason, 'requested_at', r.created_at,
            'decided_by', d.full_name, 'decided_at', r.decided_at,
            'decision_comment', r.decision_comment, 'cancel_requested_at', r.cancel_requested_at,
            'cancel_reason', r.cancel_reason,
            'notified', (SELECT COALESCE(jsonb_agg(n.email ORDER BY n.email), '[]')
                         FROM request_notifications n
                         WHERE n.request_kind = 'leave' AND n.request_id = r.id),
            'attachment', (SELECT a.file_name FROM attachments a WHERE a.request_id = r.id))
            ORDER BY r.from_date, r.created_at), '[]')
         FROM leave_requests r JOIN leave_types t ON t.id = r.leave_type_id
         LEFT JOIN users d ON d.id = r.decided_by WHERE r.user_id = $1",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    let comp_offs: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', r.id, 'status', r.status, 'reason', r.reason, 'requested_at', r.created_at,
            'decided_by', d.full_name, 'decided_at', r.decided_at,
            'decision_comment', r.decision_comment,
            'dates', (SELECT jsonb_agg(jsonb_build_object('date', x.worked_date, 'portion', x.portion)
                                       ORDER BY x.worked_date)
                      FROM comp_off_request_dates x WHERE x.comp_off_request_id = r.id))
            ORDER BY r.created_at), '[]')
         FROM comp_off_requests r LEFT JOIN users d ON d.id = r.decided_by WHERE r.user_id = $1",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    let credits: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'worked_date', c.worked_date, 'days', c.days, 'days_used', c.days_used,
            'days_expired', c.days_expired, 'expires_on', c.expires_on,
            'revoked_at', c.revoked_at, 'revoke_reason', c.revoke_reason)
            ORDER BY c.expires_on), '[]')
         FROM comp_off_credits c WHERE c.user_id = $1",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    let ledger: Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'leave_type', t.code, 'leave_year', l.leave_year, 'delta', l.delta,
            'reason', l.reason, 'note', l.note, 'created_at', l.created_at)
            ORDER BY l.created_at, l.id), '[]')
         FROM balance_ledger l JOIN leave_types t ON t.id = l.leave_type_id WHERE l.user_id = $1",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await?;
    // Numbers come back from Postgres as JSON numbers; keep them as such.
    Ok(json!({
        "generated_at": now,
        "profile": profile,
        "leave_requests": leave,
        "comp_off_requests": comp_offs,
        "comp_off_credits": credits,
        "ledger": ledger,
    }))
}

/// What anonymising found.
#[derive(Debug)]
pub enum Anonymise {
    NotFound,
    NotDeactivated,
    AlreadyDone,
    /// Done; these stored attachment objects must now be deleted.
    Done {
        storage_keys: Vec<String>,
    },
}

/// The JSON-escaped form of `s` (without the quotes), as it appears inside a JSON document.
fn json_escaped(s: &str) -> String {
    let quoted = serde_json::to_string(s).unwrap_or_default();
    quoted[1..quoted.len() - 1].to_string()
}

/// Replace the person's name and email everywhere, delete their reasons, comments and
/// attachments, keep dates, days and the ledger (FR-045).
pub async fn anonymise(
    conn: &mut PgConnection,
    user: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<Anonymise> {
    let row: Option<(String, String, String, bool)> = sqlx::query_as(
        "SELECT full_name, email::text, status::text, anonymised_at IS NOT NULL
         FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(user)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((name, email, status, done)) = row else {
        return Ok(Anonymise::NotFound);
    };
    if done {
        return Ok(Anonymise::AlreadyDone);
    }
    if status != "deactivated" {
        return Ok(Anonymise::NotDeactivated);
    }
    let short = &user.simple().to_string()[..8];
    let new_name = format!("Former employee {short}");
    let new_email = format!("anonymised-{}@anonymised.invalid", user.simple());
    sqlx::query(
        "UPDATE users SET full_name = $2, email = $3, employee_code = NULL, department = NULL,
                last_notify_list = '[]', anonymised_at = $4
         WHERE id = $1",
    )
    .bind(user)
    .bind(&new_name)
    .bind(&new_email)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    const REMOVED: &str = "[removed]";
    sqlx::query(
        "UPDATE leave_requests SET reason = $2, cancel_reason = NULL, decision_comment = NULL
         WHERE user_id = $1",
    )
    .bind(user)
    .bind(REMOVED)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE comp_off_requests SET reason = $2, decision_comment = NULL WHERE user_id = $1",
    )
    .bind(user)
    .bind(REMOVED)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE comp_off_credits SET revoke_reason = $2 WHERE user_id = $1 AND revoke_reason IS NOT NULL")
        .bind(user)
        .bind(REMOVED)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE request_notifications SET email = $3 WHERE user_id = $1 OR email = $2::citext",
    )
    .bind(user)
    .bind(&email)
    .bind(&new_email)
    .execute(&mut *conn)
    .await?;
    let storage_keys: Vec<String> = sqlx::query_scalar(
        "DELETE FROM attachments WHERE owner_user_id = $1 RETURNING storage_key",
    )
    .bind(user)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user)
        .execute(&mut *conn)
        .await?;
    // Audit records stay (FR-045): drop free text about the person's own requests, then rewrite
    // their name and email wherever they appear.
    sqlx::query(
        "UPDATE audit_log
         SET before = before - 'reason' - 'comment' - 'cancel_reason',
             after = after - 'reason' - 'comment' - 'cancel_reason'
         WHERE entity_id IN (SELECT id FROM leave_requests WHERE user_id = $1
                             UNION ALL SELECT id FROM comp_off_requests WHERE user_id = $1)",
    )
    .bind(user)
    .execute(&mut *conn)
    .await?;
    for (old, new) in [
        (json_escaped(&email), json_escaped(&new_email)),
        (json_escaped(&name), json_escaped(&new_name)),
    ] {
        if old.is_empty() {
            continue;
        }
        sqlx::query(
            "UPDATE audit_log
             SET before = replace(before::text, $1, $2)::jsonb,
                 after = replace(after::text, $1, $2)::jsonb
             WHERE strpos(COALESCE(before::text, ''), $1) > 0
                OR strpos(COALESCE(after::text, ''), $1) > 0",
        )
        .bind(&old)
        .bind(&new)
        .execute(&mut *conn)
        .await?;
    }
    Ok(Anonymise::Done { storage_keys })
}

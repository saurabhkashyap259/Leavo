//! `CurrentUser`: resolves the `leavo_sid` cookie to an active user, slides the session
//! expiry, and records the org on the request span (FR-005, FR-006).
use crate::app::AppState;
use crate::error::ApiError;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::CookieJar;
use leavo_core::error::ErrorCode;
use leavo_core::permissions::{Actor, Role};
use leavo_core::tokens::{hash_token, slide};
use leavo_db::sessions;
use leavo_db::tx::begin_org_tx;
use uuid::Uuid;

pub const COOKIE: &str = "leavo_sid";

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: Uuid,
    pub org_id: Uuid,
    pub role: Role,
    pub session_id: Uuid,
}

impl CurrentUser {
    pub fn actor(&self) -> Actor {
        Actor {
            user_id: self.id,
            role: self.role,
        }
    }
}

pub fn parse_role(s: &str) -> Role {
    match s {
        "admin" => Role::Admin,
        "approver" => Role::Approver,
        _ => Role::Employee,
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let unauth = || ApiError::new(ErrorCode::Unauthenticated);
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(COOKIE)
            .map(|c| c.value().to_string())
            .ok_or_else(unauth)?;
        let now = state.clock.now();
        let session = sessions::find_active(&state.db, &hash_token(&token), now)
            .await?
            .ok_or_else(unauth)?;

        let mut tx = begin_org_tx(&state.db, session.org_id).await?;
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT role::text, status::text FROM users WHERE id = $1")
                .bind(session.user_id)
                .fetch_optional(&mut *tx)
                .await?;
        let (role, status) = row.ok_or_else(unauth)?;
        if status != "active" {
            return Err(unauth());
        }
        if let Some(next) = slide(
            session.expires_at,
            session.absolute_expires_at,
            session.keep_signed_in,
            now,
        ) {
            sessions::extend(&mut *tx, session.id, next, now).await?;
        }
        tx.commit().await?;

        tracing::Span::current().record("org_id", tracing::field::display(session.org_id));
        Ok(CurrentUser {
            id: session.user_id,
            org_id: session.org_id,
            role: parse_role(&role),
            session_id: session.id,
        })
    }
}

impl CurrentUser {
    /// `403 FORBIDDEN` unless the user's role allows `action`.
    pub fn require(&self, action: leavo_core::permissions::Action) -> Result<(), ApiError> {
        if leavo_core::permissions::can(&self.actor(), action) {
            Ok(())
        } else {
            Err(ApiError::new(ErrorCode::Forbidden))
        }
    }
}

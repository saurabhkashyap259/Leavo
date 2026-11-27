//! Role-based permissions (ERD §5). Every handler asks [`can`] before acting.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Employee,
    Approver,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    pub user_id: Uuid,
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ViewOwnData,
    RequestLeave,
    ViewWhosAway,
    /// Leave type, reason and attachments of `subject`'s leave (FR-058).
    ViewLeaveDetails {
        subject: Uuid,
        eligible_approver: bool,
    },
    /// Approve or reject a request made by `requester` (FR-022, FR-023).
    Decide {
        requester: Uuid,
        eligible_approver: bool,
    },
    ManagePolicy,
    ManageHolidays,
    ManageUsers,
    AdjustBalances,
    ViewReports,
    ViewAuditLog,
    ImportData,
    ManageInstance,
}

pub fn can(actor: &Actor, action: Action) -> bool {
    use Action::*;
    match action {
        ViewOwnData | RequestLeave | ViewWhosAway => true,
        ViewLeaveDetails {
            subject,
            eligible_approver,
        } => {
            subject == actor.user_id
                || actor.role == Role::Admin
                || (actor.role == Role::Approver && eligible_approver)
        }
        Decide {
            requester,
            eligible_approver,
        } => {
            requester != actor.user_id
                && (actor.role == Role::Admin
                    || (actor.role == Role::Approver && eligible_approver))
        }
        ManagePolicy | ManageHolidays | ManageUsers | AdjustBalances | ViewReports
        | ViewAuditLog | ImportData | ManageInstance => actor.role == Role::Admin,
    }
}

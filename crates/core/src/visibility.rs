//! Who may see the leave type and reason of someone else's leave (FR-058, research R10).
//!
//! The same rule serves request-level views (approvals inbox, request detail, emails), where
//! the caller passes the request's `routed_group_id`, and person-level views (who's away, team
//! calendar, dashboards), where it passes the person's current approver group.
use crate::permissions::Role;
use uuid::Uuid;

/// The signed-in user looking at someone's leave.
#[derive(Debug, Clone, Copy)]
pub struct Viewer<'a> {
    pub id: Uuid,
    pub role: Role,
    /// Approver groups the viewer is a member of.
    pub approves: &'a [Uuid],
}

/// Whose leave is shown, and which approver group decides their requests.
#[derive(Debug, Clone, Copy)]
pub struct Subject {
    pub user: Uuid,
    /// `None` = no group: every approver is eligible (FR-022).
    pub group: Option<Uuid>,
}

/// True for the person themselves, admins, and the subject's eligible approvers.
pub fn can_see_details(viewer: &Viewer<'_>, subject: &Subject) -> bool {
    if viewer.id == subject.user {
        return true;
    }
    match viewer.role {
        Role::Admin => true,
        Role::Approver => subject.group.is_none_or(|g| viewer.approves.contains(&g)),
        // Group membership left over after a demotion grants nothing.
        Role::Employee => false,
    }
}

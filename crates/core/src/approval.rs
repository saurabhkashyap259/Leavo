//! Approval routing (FR-022) and request state transitions (FR-018, FR-020, FR-023).
use crate::error::ErrorCode;
use uuid::Uuid;

/// Inputs for routing. Callers pass only active users.
pub struct Routing<'a> {
    pub requester: Uuid,
    /// Approvers and admins in the requester's approver group; `None` when they have no group.
    pub group_members: Option<&'a [Uuid]>,
    /// Every approver in the org (used when the requester has no group).
    pub approvers: &'a [Uuid],
    pub admins: &'a [Uuid],
}

/// The request's eligible approvers: the group's members (or all approvers without a group)
/// plus every admin, never the requester. Sorted, without duplicates.
pub fn route_approvers(r: &Routing<'_>) -> Result<Vec<Uuid>, ErrorCode> {
    let base = r.group_members.unwrap_or(r.approvers);
    let mut out: Vec<Uuid> = base
        .iter()
        .chain(r.admins)
        .copied()
        .filter(|id| *id != r.requester)
        .collect();
    out.sort();
    out.dedup();
    if out.is_empty() {
        return Err(ErrorCode::NoApproverAvailable);
    }
    Ok(out)
}

/// `leave_requests.status` (data-model.md state diagram).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
    CancelRequested,
}

impl RequestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::CancelRequested => "cancel_requested",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "approved" => Self::Approved,
            "rejected" => Self::Rejected,
            "cancelled" => Self::Cancelled,
            "cancel_requested" => Self::CancelRequested,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestEvent {
    Approve,
    Reject,
    /// The employee withdraws a pending request.
    Withdraw,
    /// The employee cancels approved leave that has not started.
    CancelBeforeStart,
    /// The employee asks to cancel approved leave that has started.
    RequestCancel,
    AcceptCancel,
    DeclineCancel,
}

/// The status after `event`, or `ALREADY_DECIDED` when the request is not in a state that
/// allows it.
pub fn transition(from: RequestStatus, event: RequestEvent) -> Result<RequestStatus, ErrorCode> {
    use RequestEvent as E;
    use RequestStatus as S;
    match (from, event) {
        (S::Pending, E::Approve) => Ok(S::Approved),
        (S::Pending, E::Reject) => Ok(S::Rejected),
        (S::Pending, E::Withdraw) => Ok(S::Cancelled),
        (S::Approved, E::CancelBeforeStart) => Ok(S::Cancelled),
        (S::Approved, E::RequestCancel) => Ok(S::CancelRequested),
        (S::CancelRequested, E::AcceptCancel) => Ok(S::Cancelled),
        (S::CancelRequested, E::DeclineCancel) => Ok(S::Approved),
        _ => Err(ErrorCode::AlreadyDecided),
    }
}

/// What cancelling a request in `status` means today (FR-020): a pending request is withdrawn;
/// approved leave is cancelled outright before its first day and needs an approver from then on.
pub fn cancel_event(
    status: RequestStatus,
    from: chrono::NaiveDate,
    today: chrono::NaiveDate,
) -> Result<RequestEvent, ErrorCode> {
    let event = match status {
        RequestStatus::Pending => RequestEvent::Withdraw,
        RequestStatus::Approved if today < from => RequestEvent::CancelBeforeStart,
        RequestStatus::Approved => RequestEvent::RequestCancel,
        _ => return Err(ErrorCode::AlreadyDecided),
    };
    transition(status, event).map(|_| event)
}

/// Days a cancellation returns, per leave year in year order (FR-020, FR-055): only days dated on
/// or after `requested_on`; earlier days stay taken. Years with nothing to return are left out.
pub fn days_to_return(
    days: &[crate::calendar::BookedDay],
    requested_on: chrono::NaiveDate,
) -> Vec<(i32, rust_decimal::Decimal)> {
    let mut out: std::collections::BTreeMap<i32, rust_decimal::Decimal> = Default::default();
    for d in days.iter().filter(|d| d.day >= requested_on) {
        *out.entry(d.leave_year).or_default() += d.portion;
    }
    out.into_iter().collect()
}

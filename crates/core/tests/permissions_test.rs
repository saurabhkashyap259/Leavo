//! T015 — permissions matrix (ERD §5) and "never approve own request" (FR-022).
use leavo_core::permissions::{can, Action, Actor, Role};
use uuid::Uuid;

fn actor(role: Role) -> Actor {
    Actor {
        user_id: Uuid::now_v7(),
        role,
    }
}

#[test]
fn everyone_sees_own_data_and_whos_away() {
    for role in [Role::Employee, Role::Approver, Role::Admin] {
        let a = actor(role);
        assert!(can(&a, Action::ViewOwnData));
        assert!(can(&a, Action::ViewWhosAway));
        assert!(can(&a, Action::RequestLeave));
    }
}

#[test]
fn leave_details_of_others_follow_eligibility() {
    let emp = actor(Role::Employee);
    let appr = actor(Role::Approver);
    let admin = actor(Role::Admin);
    let other = Uuid::now_v7();
    let details = |eligible| Action::ViewLeaveDetails {
        subject: other,
        eligible_approver: eligible,
    };
    assert!(!can(&emp, details(false)));
    assert!(
        !can(&emp, details(true)),
        "employees are never eligible approvers"
    );
    assert!(!can(&appr, details(false)));
    assert!(can(&appr, details(true)));
    assert!(can(&admin, details(false)));
}

#[test]
fn own_leave_details_are_always_visible() {
    let emp = actor(Role::Employee);
    assert!(can(
        &emp,
        Action::ViewLeaveDetails {
            subject: emp.user_id,
            eligible_approver: false
        }
    ));
}

#[test]
fn deciding_requires_eligibility_and_never_own_request() {
    let appr = actor(Role::Approver);
    let admin = actor(Role::Admin);
    let emp = actor(Role::Employee);
    let req = Uuid::now_v7();
    assert!(can(
        &appr,
        Action::Decide {
            requester: req,
            eligible_approver: true
        }
    ));
    assert!(!can(
        &appr,
        Action::Decide {
            requester: req,
            eligible_approver: false
        }
    ));
    assert!(can(
        &admin,
        Action::Decide {
            requester: req,
            eligible_approver: false
        }
    ));
    assert!(!can(
        &emp,
        Action::Decide {
            requester: req,
            eligible_approver: true
        }
    ));
    assert!(!can(
        &appr,
        Action::Decide {
            requester: appr.user_id,
            eligible_approver: true
        }
    ));
    assert!(!can(
        &admin,
        Action::Decide {
            requester: admin.user_id,
            eligible_approver: true
        }
    ));
}

#[test]
fn admin_only_actions() {
    let admin_only = [
        Action::ManagePolicy,
        Action::ManageHolidays,
        Action::ManageUsers,
        Action::AdjustBalances,
        Action::ViewReports,
        Action::ViewAuditLog,
        Action::ImportData,
        Action::ManageInstance,
    ];
    for action in admin_only {
        assert!(can(&actor(Role::Admin), action), "{action:?}");
        assert!(!can(&actor(Role::Approver), action), "{action:?}");
        assert!(!can(&actor(Role::Employee), action), "{action:?}");
    }
}

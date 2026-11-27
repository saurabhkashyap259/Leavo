//! T125 — who may see the leave type and reason of someone else's leave (FR-058).
use leavo_core::permissions::Role;
use leavo_core::visibility::{can_see_details, Subject, Viewer};
use uuid::Uuid;

fn id() -> Uuid {
    Uuid::now_v7()
}

#[test]
fn admins_see_everyone() {
    let admin = id();
    let viewer = Viewer {
        id: admin,
        role: Role::Admin,
        approves: &[],
    };
    for group in [None, Some(id())] {
        assert!(can_see_details(&viewer, &Subject { user: id(), group }));
    }
}

#[test]
fn approvers_see_the_people_of_their_groups() {
    let (eng, sales) = (id(), id());
    let priya = Viewer {
        id: id(),
        role: Role::Approver,
        approves: &[eng],
    };
    assert!(can_see_details(
        &priya,
        &Subject {
            user: id(),
            group: Some(eng)
        }
    ));
    assert!(!can_see_details(
        &priya,
        &Subject {
            user: id(),
            group: Some(sales)
        }
    ));
}

#[test]
fn every_approver_sees_people_without_a_group() {
    // FR-022: requests of people without a group go to every approver.
    let lone = Viewer {
        id: id(),
        role: Role::Approver,
        approves: &[],
    };
    assert!(can_see_details(
        &lone,
        &Subject {
            user: id(),
            group: None
        }
    ));
}

#[test]
fn employees_see_only_their_own() {
    let me = id();
    // Membership left over after a demotion grants nothing: the role decides.
    let group = id();
    let viewer = Viewer {
        id: me,
        role: Role::Employee,
        approves: &[group],
    };
    assert!(can_see_details(
        &viewer,
        &Subject {
            user: me,
            group: Some(group)
        }
    ));
    for g in [None, Some(group)] {
        assert!(!can_see_details(
            &viewer,
            &Subject {
                user: id(),
                group: g
            }
        ));
    }
}

#[test]
fn the_group_passed_decides_request_vs_person_level() {
    // Asha moved from Eng to Sales with a request still pending. For that request the caller
    // passes its routed group (Eng); for person-level views her current group (Sales).
    let (eng, sales, asha) = (id(), id(), id());
    let priya = Viewer {
        id: id(),
        role: Role::Approver,
        approves: &[eng],
    };
    let request_level = Subject {
        user: asha,
        group: Some(eng),
    };
    let person_level = Subject {
        user: asha,
        group: Some(sales),
    };
    assert!(can_see_details(&priya, &request_level));
    assert!(!can_see_details(&priya, &person_level));
}

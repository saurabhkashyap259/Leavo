//! T091 — who receives a request (FR-022).
use leavo_core::approval::{route_approvers, Routing};
use leavo_core::error::ErrorCode;
use uuid::Uuid;

fn ids(n: usize) -> Vec<Uuid> {
    let mut v: Vec<Uuid> = (0..n).map(|_| Uuid::now_v7()).collect();
    v.sort();
    v
}

fn sorted(mut v: Vec<Uuid>) -> Vec<Uuid> {
    v.sort();
    v
}

#[test]
fn group_approvers_and_admins_minus_requester() {
    let u = ids(6);
    let (requester, g1, g2, other_approver, admin1, admin2) = (u[0], u[1], u[2], u[3], u[4], u[5]);
    let got = route_approvers(&Routing {
        requester,
        group_members: Some(&[g1, g2, requester]),
        approvers: &[g1, g2, other_approver],
        admins: &[admin1, admin2],
    })
    .unwrap();
    assert_eq!(got, sorted(vec![g1, g2, admin1, admin2]));
    assert!(!got.contains(&other_approver));
}

#[test]
fn no_group_goes_to_all_approvers_and_admins() {
    let u = ids(4);
    let got = route_approvers(&Routing {
        requester: u[0],
        group_members: None,
        approvers: &[u[1], u[2]],
        admins: &[u[3]],
    })
    .unwrap();
    assert_eq!(got, sorted(vec![u[1], u[2], u[3]]));
}

#[test]
fn an_admin_who_is_also_a_group_member_appears_once() {
    let u = ids(2);
    let got = route_approvers(&Routing {
        requester: u[0],
        group_members: Some(&[u[1]]),
        approvers: &[],
        admins: &[u[1]],
    })
    .unwrap();
    assert_eq!(got, vec![u[1]]);
}

#[test]
fn an_admin_requesting_goes_to_the_other_admins() {
    let u = ids(2);
    let got = route_approvers(&Routing {
        requester: u[0],
        group_members: None,
        approvers: &[],
        admins: &[u[0], u[1]],
    })
    .unwrap();
    assert_eq!(got, vec![u[1]]);
}

#[test]
fn nobody_left_is_no_approver_available() {
    let u = ids(1);
    let err = route_approvers(&Routing {
        requester: u[0],
        group_members: Some(&[u[0]]),
        approvers: &[u[0]],
        admins: &[u[0]],
    })
    .unwrap_err();
    assert_eq!(err, ErrorCode::NoApproverAvailable);
}

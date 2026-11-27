//! T110 — every allowed and forbidden transition of a leave request (data-model.md states).
use leavo_core::approval::{transition, RequestEvent as E, RequestStatus as S};
use leavo_core::error::ErrorCode;

const STATES: [S; 5] = [
    S::Pending,
    S::Approved,
    S::Rejected,
    S::Cancelled,
    S::CancelRequested,
];
const EVENTS: [E; 7] = [
    E::Approve,
    E::Reject,
    E::Withdraw,
    E::CancelBeforeStart,
    E::RequestCancel,
    E::AcceptCancel,
    E::DeclineCancel,
];

/// The complete set of allowed transitions; everything else is refused.
const ALLOWED: [(S, E, S); 7] = [
    (S::Pending, E::Approve, S::Approved),
    (S::Pending, E::Reject, S::Rejected),
    (S::Pending, E::Withdraw, S::Cancelled),
    (S::Approved, E::CancelBeforeStart, S::Cancelled),
    (S::Approved, E::RequestCancel, S::CancelRequested),
    (S::CancelRequested, E::AcceptCancel, S::Cancelled),
    (S::CancelRequested, E::DeclineCancel, S::Approved),
];

#[test]
fn transition_table() {
    for from in STATES {
        for ev in EVENTS {
            let expected = ALLOWED
                .iter()
                .find(|(f, e, _)| *f == from && *e == ev)
                .map(|(_, _, to)| *to);
            match expected {
                Some(to) => assert_eq!(transition(from, ev), Ok(to), "{from:?} --{ev:?}"),
                None => assert_eq!(
                    transition(from, ev),
                    Err(ErrorCode::AlreadyDecided),
                    "{from:?} --{ev:?} must be refused"
                ),
            }
        }
    }
}

#[test]
fn final_states_accept_nothing() {
    for from in [S::Rejected, S::Cancelled] {
        for ev in EVENTS {
            assert!(transition(from, ev).is_err());
        }
    }
}

#[test]
fn status_names_match_the_database_enum() {
    let names: Vec<&str> = STATES.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        names,
        [
            "pending",
            "approved",
            "rejected",
            "cancelled",
            "cancel_requested"
        ]
    );
    for s in STATES {
        assert_eq!(S::parse(s.as_str()), Some(s));
    }
    assert_eq!(S::parse("nope"), None);
}

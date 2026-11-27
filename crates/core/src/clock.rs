//! Time source abstraction. Every time-based rule takes a [`Clock`] so tests can pin "now".
use chrono::{DateTime, Duration, Utc};
use std::sync::Mutex;

pub trait Clock: Send + Sync + std::fmt::Debug {
    fn now(&self) -> DateTime<Utc>;
}

/// Real wall-clock time.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock that only moves when told to. Used in tests.
#[derive(Debug)]
pub struct FixedClock(Mutex<DateTime<Utc>>);

impl FixedClock {
    pub fn new(at: DateTime<Utc>) -> Self {
        Self(Mutex::new(at))
    }

    pub fn set(&self, at: DateTime<Utc>) {
        *self.0.lock().expect("clock lock") = at;
    }

    pub fn advance(&self, by: Duration) {
        let mut t = self.0.lock().expect("clock lock");
        *t += by;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().expect("clock lock")
    }
}

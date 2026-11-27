//! PostgreSQL access: migrations, org-scoped transactions, repositories.
pub mod approvals_repo;
pub mod audit;
pub mod auth_repo;
pub mod away_repo;
pub mod comp_off_repo;
pub mod holidays_repo;
pub mod import_repo;
pub mod jobs;
pub mod leave_types_repo;
pub mod leaves_repo;
pub mod ledger;
pub mod migrate;
pub mod orgs_repo;
pub mod outbox;
pub mod personal_data;
pub mod pool;
pub mod rollover_repo;
pub mod seed;
pub mod sessions;
pub mod tx;
pub mod users_repo;

#[cfg(feature = "testkit")]
pub mod testkit;

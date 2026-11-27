//! `/api/v1` routes.
use crate::app::AppState;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, patch, post};
use axum::Router;

pub mod admin_audit;
pub mod admin_balances;
pub mod admin_dashboard;
pub mod admin_groups;
pub mod admin_import;
pub mod admin_instance;
pub mod admin_leave_types;
pub mod admin_org;
pub mod admin_reports;
pub mod admin_users;
pub mod approvals;
pub mod attachments;
pub mod auth;
pub mod away;
pub mod cancel;
pub mod comp_offs;
pub mod holidays;
pub mod leaves;
pub mod me;
pub mod orgs;
pub mod personal_data;
pub mod setup;
pub mod users;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/me", get(me::get_me).patch(me::patch_me))
        .route("/me/balances", get(me::balances))
        .route("/me/leaves", get(me::leaves))
        .route("/me/export", get(personal_data::my_export))
        .route(
            "/admin/users",
            get(admin_users::list).post(admin_users::invite),
        )
        .route("/admin/users/import", post(admin_users::import))
        .route(
            "/admin/users/{id}",
            get(admin_users::get).patch(admin_users::update),
        )
        .route(
            "/admin/users/{id}/export",
            post(personal_data::admin_export),
        )
        .route(
            "/admin/users/{id}/anonymise",
            post(personal_data::anonymise),
        )
        .route("/admin/users/{id}/balances", get(admin_balances::balances))
        .route(
            "/admin/users/{id}/adjustments",
            post(admin_balances::adjust),
        )
        .route(
            "/admin/approver-groups",
            get(admin_groups::list).post(admin_groups::create),
        )
        .route(
            "/admin/approver-groups/{id}",
            patch(admin_groups::update).delete(admin_groups::remove),
        )
        .route("/admin/dashboard", get(admin_dashboard::dashboard))
        .route("/admin/reports/leaves", get(admin_reports::leaves))
        .route("/admin/reports/leaves.csv", get(admin_reports::leaves_csv))
        .route("/admin/audit-log", get(admin_audit::list))
        .route("/admin/import/{kind}", post(admin_import::import))
        .route("/admin/instance", get(admin_instance::status))
        .route(
            "/admin/instance/test-email",
            post(admin_instance::test_email),
        )
        .route("/admin/org/sign-out-all", post(admin_org::sign_out_all))
        .route("/me/leaves.csv", get(me::leaves_csv))
        .route("/users/search", get(users::search))
        .route("/away", get(away::away))
        .route("/team/calendar", get(away::team_calendar))
        .route("/leaves/preview", post(leaves::preview))
        .route("/leaves", post(leaves::create))
        .route("/leaves/{id}", get(leaves::get))
        .route("/leaves/{id}/cancel", post(cancel::cancel))
        .route(
            "/comp-offs",
            get(comp_offs::summary).post(comp_offs::create),
        )
        .route("/comp-offs/{id}/cancel", post(comp_offs::cancel))
        .route(
            "/admin/comp-off-credits/{id}/revoke",
            post(comp_offs::revoke),
        )
        .route("/approvals", get(approvals::inbox))
        .route("/approvals/{kind}/{id}/decide", post(approvals::decide))
        .route("/approvals/action/{token}", get(approvals::action_link))
        .route(
            "/attachments",
            // The 5 MB cap is enforced while streaming; this only bounds the whole body.
            post(attachments::upload).layer(DefaultBodyLimit::max(6 * 1024 * 1024)),
        )
        .route("/attachments/{id}/url", get(attachments::signed_url))
        .route("/attachments/{id}/download", get(attachments::download))
        .route("/auth/code", post(auth::request_code))
        .route("/auth/verify", post(auth::verify))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/recovery/{link}", get(crate::auth::recovery::redeem))
        .route("/setup/status", get(setup::status))
        .route("/setup/verify-token", post(setup::verify_token))
        .route("/setup/test-email", post(setup::test_email))
        .route("/setup/complete", post(setup::complete))
        .route("/orgs", post(orgs::register))
        .route("/holiday-packs", get(holidays::packs))
        .route("/holidays", get(holidays::list))
        .route("/admin/holidays", post(holidays::create))
        .route("/admin/holidays/import", post(holidays::import))
        .route(
            "/admin/holidays/{id}",
            patch(holidays::update).delete(holidays::remove),
        )
        .route(
            "/admin/org",
            get(admin_org::get_org).patch(admin_org::patch_org),
        )
        .route(
            "/admin/org/domains",
            get(admin_org::list_domains).post(admin_org::add_domain),
        )
        .route("/admin/org/domains/{id}", delete(admin_org::remove_domain))
        .route(
            "/admin/leave-types",
            get(admin_leave_types::list).post(admin_leave_types::create),
        )
        .route("/admin/leave-types/{id}", patch(admin_leave_types::patch))
        .route(
            "/admin/carry-forward/preview",
            get(admin_leave_types::carry_forward_preview),
        )
        .route(
            "/admin/carry-forward/preview.csv",
            get(admin_leave_types::carry_forward_csv),
        )
        .route(
            "/admin/leave-types/{id}/grants",
            get(admin_leave_types::list_grants).post(admin_leave_types::grant),
        )
        .route(
            "/admin/leave-types/{id}/grants/{user_id}",
            delete(admin_leave_types::revoke),
        )
}

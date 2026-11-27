-- T035: append-only balance ledger, ledger-only balance view, rollover runs, audit log, jobs.

CREATE TYPE ledger_reason AS ENUM (
    'allotment', 'prorate', 'quota_change', 'carry_forward', 'negative_carry', 'lapse',
    'leave_taken', 'leave_reversed', 'comp_off_credit', 'comp_off_expired', 'comp_off_revoked',
    'manual_adjustment', 'import'
);

CREATE TABLE balance_ledger (
    id             uuid PRIMARY KEY,
    org_id         uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    leave_type_id  uuid NOT NULL REFERENCES leave_types(id) ON DELETE CASCADE,
    leave_year     int NOT NULL CHECK (leave_year BETWEEN 2000 AND 2200),
    delta          numeric(6,1) NOT NULL CHECK (delta <> 0 AND delta * 2 = trunc(delta * 2)),
    reason         ledger_reason NOT NULL,
    ref_kind       text,
    ref_id         uuid,
    note           text,
    created_by     uuid REFERENCES users(id),
    created_at     timestamptz NOT NULL DEFAULT now(),
    CHECK (reason <> 'manual_adjustment' OR (note IS NOT NULL AND btrim(note) <> ''))
);
CREATE INDEX balance_ledger_user_type_year_idx ON balance_ledger (user_id, leave_type_id, leave_year);

-- Append-only for the application (constitution V).
REVOKE UPDATE, DELETE, TRUNCATE ON balance_ledger FROM leavo_app;

ALTER TABLE balance_ledger ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON balance_ledger USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());

-- Ledger-only sums. Comp-off is one balance across leave years (leave_year NULL).
-- `v_balances` (pending/available) is added with leave requests in 0006.
CREATE VIEW v_ledger_balances WITH (security_invoker = true) AS
SELECT
    l.org_id,
    l.user_id,
    l.leave_type_id,
    CASE WHEN t.is_comp_off THEN NULL ELSE l.leave_year END AS leave_year,
    COALESCE(SUM(l.delta) FILTER (WHERE l.reason IN ('allotment', 'prorate', 'quota_change', 'import')), 0) AS allotted,
    COALESCE(SUM(l.delta) FILTER (WHERE l.reason IN ('carry_forward', 'negative_carry')), 0) AS carried,
    COALESCE(-SUM(l.delta) FILTER (WHERE l.reason IN ('leave_taken', 'leave_reversed')), 0) AS taken,
    COALESCE(SUM(l.delta) FILTER (WHERE l.reason IN ('manual_adjustment', 'comp_off_credit', 'comp_off_expired', 'comp_off_revoked', 'lapse')), 0) AS adjusted,
    SUM(l.delta) AS balance
FROM balance_ledger l
JOIN leave_types t ON t.id = l.leave_type_id
GROUP BY l.org_id, l.user_id, l.leave_type_id, CASE WHEN t.is_comp_off THEN NULL ELSE l.leave_year END;

CREATE TABLE rollover_runs (
    org_id       uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    leave_year   int NOT NULL,
    started_at   timestamptz NOT NULL DEFAULT now(),
    finished_at  timestamptz,
    summary      jsonb NOT NULL DEFAULT '{}',
    PRIMARY KEY (org_id, leave_year)
);
ALTER TABLE rollover_runs ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON rollover_runs USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());

CREATE TABLE audit_log (
    id           uuid PRIMARY KEY,
    org_id       uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    actor_id     uuid,
    action       text NOT NULL,
    entity_kind  text NOT NULL,
    entity_id    uuid,
    before       jsonb,
    after        jsonb,
    ip           inet,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_org_time_idx ON audit_log (org_id, created_at DESC);
ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON audit_log USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());

-- Job queue / outbox. Not RLS-scoped: the worker claims jobs for every org and
-- each handler opens its own org-scoped transaction from `org_id`.
CREATE TYPE job_status AS ENUM ('queued', 'running', 'done', 'dead');

CREATE TABLE jobs (
    id            uuid PRIMARY KEY,
    kind          text NOT NULL,
    org_id        uuid REFERENCES organizations(id) ON DELETE CASCADE,
    payload       jsonb NOT NULL DEFAULT '{}',
    run_at        timestamptz NOT NULL DEFAULT now(),
    attempts      int NOT NULL DEFAULT 0,
    max_attempts  int NOT NULL DEFAULT 5,
    status        job_status NOT NULL DEFAULT 'queued',
    locked_by     text,
    locked_at     timestamptz,
    last_error    text,
    dedupe_key    text UNIQUE,
    created_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz
);
CREATE INDEX jobs_ready_idx ON jobs (run_at) WHERE status = 'queued';

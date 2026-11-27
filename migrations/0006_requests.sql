-- T099: leave requests, booked days, attachments, notified members, idempotency keys and the
-- `v_balances` view (ledger + pending days per leave year).

CREATE TYPE request_status AS ENUM ('pending', 'approved', 'rejected', 'cancelled', 'cancel_requested');
CREATE TYPE day_half AS ENUM ('first', 'second');

CREATE TABLE leave_requests (
    id                   uuid PRIMARY KEY,
    org_id               uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id              uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    leave_type_id        uuid NOT NULL REFERENCES leave_types(id),
    from_date            date NOT NULL,
    to_date              date NOT NULL,
    start_half           day_half,
    end_half             day_half,
    days                 numeric(5,1) NOT NULL CHECK (days > 0 AND days * 2 = trunc(days * 2)),
    reason               text NOT NULL CHECK (char_length(btrim(reason)) BETWEEN 1 AND 500),
    status               request_status NOT NULL DEFAULT 'pending',
    -- Requester's approver group when submitted; fixes the eligible approvers (FR-022, FR-058).
    routed_group_id      uuid REFERENCES approver_groups(id) ON DELETE SET NULL,
    decided_by           uuid REFERENCES users(id),
    decided_at           timestamptz,
    decision_comment     text,
    cancel_requested_at  timestamptz,
    cancel_reason        text,
    version              int NOT NULL DEFAULT 1,
    created_at           timestamptz NOT NULL DEFAULT now(),
    updated_at           timestamptz NOT NULL DEFAULT now(),
    CHECK (to_date >= from_date)
);
CREATE INDEX leave_requests_user_idx ON leave_requests (user_id, from_date);
CREATE INDEX leave_requests_org_status_idx ON leave_requests (org_id, status, created_at);

CREATE TABLE leave_request_days (
    id                uuid PRIMARY KEY,
    leave_request_id  uuid NOT NULL REFERENCES leave_requests(id) ON DELETE CASCADE,
    org_id            uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id           uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day               date NOT NULL,
    leave_year        int NOT NULL CHECK (leave_year BETWEEN 2000 AND 2200),
    portion           numeric(2,1) NOT NULL CHECK (portion IN (0.5, 1.0)),
    half              day_half,
    active            boolean NOT NULL DEFAULT true,
    CHECK ((half IS NULL) = (portion = 1.0))
);
-- The same day (or the same half) can be booked only once (FR-019); a NULL half is the full
-- day. A full day against a half day on the same date is rejected by the engine under the
-- requester's lock. NULLS NOT DISTINCT needs PostgreSQL 15+ (plan: Storage).
CREATE UNIQUE INDEX leave_request_days_booked_idx
    ON leave_request_days (user_id, day, half) NULLS NOT DISTINCT WHERE active;
CREATE INDEX leave_request_days_request_idx ON leave_request_days (leave_request_id);

CREATE TABLE attachments (
    id             uuid PRIMARY KEY,
    org_id         uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    owner_user_id  uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    request_id     uuid REFERENCES leave_requests(id) ON DELETE SET NULL,
    storage_key    text NOT NULL UNIQUE,
    file_name      text NOT NULL CHECK (char_length(file_name) BETWEEN 1 AND 255),
    content_type   text NOT NULL,
    size_bytes     int NOT NULL CHECK (size_bytes BETWEEN 1 AND 5242880),
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX attachments_request_idx ON attachments (request_id) WHERE request_id IS NOT NULL;

CREATE TYPE request_kind AS ENUM ('leave', 'comp_off');

CREATE TABLE request_notifications (
    id            uuid PRIMARY KEY,
    org_id        uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    request_id    uuid NOT NULL,
    request_kind  request_kind NOT NULL,
    user_id       uuid REFERENCES users(id) ON DELETE SET NULL,
    email         citext NOT NULL,
    UNIQUE (request_kind, request_id, email)
);

CREATE TABLE idempotency_keys (
    org_id        uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id       uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key           text NOT NULL,
    endpoint      text NOT NULL,
    request_hash  bytea NOT NULL,
    status        smallint NOT NULL,
    response      jsonb NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, key)
);
CREATE INDEX idempotency_keys_created_idx ON idempotency_keys (created_at);

ALTER TABLE leave_requests ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON leave_requests USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE leave_request_days ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON leave_request_days USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE attachments ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON attachments USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE request_notifications ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON request_notifications USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE idempotency_keys ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON idempotency_keys USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());

-- Pending days per (user, type, leave year); comp-off is one pool across years.
CREATE VIEW v_pending_days WITH (security_invoker = true) AS
SELECT
    d.org_id,
    d.user_id,
    r.leave_type_id,
    CASE WHEN t.is_comp_off THEN NULL ELSE d.leave_year END AS leave_year,
    SUM(d.portion) AS pending
FROM leave_request_days d
JOIN leave_requests r ON r.id = d.leave_request_id
JOIN leave_types t ON t.id = r.leave_type_id
WHERE d.active AND r.status = 'pending'
GROUP BY d.org_id, d.user_id, r.leave_type_id, CASE WHEN t.is_comp_off THEN NULL ELSE d.leave_year END;

-- Ledger figures plus pending days. `available = balance − pending`. The projected allotment
-- of a leave year that has not started (FR-055) needs the org's "today" and the pro-rating
-- rules, so the application adds it (leavo_db::leaves_repo::projected_allotment).
CREATE VIEW v_balances WITH (security_invoker = true) AS
SELECT
    COALESCE(b.org_id, p.org_id) AS org_id,
    COALESCE(b.user_id, p.user_id) AS user_id,
    COALESCE(b.leave_type_id, p.leave_type_id) AS leave_type_id,
    COALESCE(b.leave_year, p.leave_year) AS leave_year,
    COALESCE(b.allotted, 0) AS allotted,
    COALESCE(b.carried, 0) AS carried,
    COALESCE(b.taken, 0) AS taken,
    COALESCE(b.adjusted, 0) AS adjusted,
    COALESCE(b.balance, 0) AS balance,
    COALESCE(p.pending, 0) AS pending,
    COALESCE(b.balance, 0) - COALESCE(p.pending, 0) AS available
FROM v_ledger_balances b
FULL JOIN v_pending_days p
       ON p.user_id = b.user_id
      AND p.leave_type_id = b.leave_type_id
      AND p.leave_year IS NOT DISTINCT FROM b.leave_year;

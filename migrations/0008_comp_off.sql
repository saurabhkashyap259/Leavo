-- T149: comp-off requests, claimed dates, credits and their use by comp-off leave
-- (FR-030–FR-032, FR-060, FR-061).

CREATE TABLE comp_off_requests (
    id                uuid PRIMARY KEY,
    org_id            uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id           uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason            text NOT NULL CHECK (char_length(btrim(reason)) BETWEEN 1 AND 500),
    -- Only pending, approved, rejected and cancelled are used.
    status            request_status NOT NULL DEFAULT 'pending'
                      CHECK (status <> 'cancel_requested'),
    -- Requester's approver group when submitted; fixes the eligible approvers (FR-022).
    routed_group_id   uuid REFERENCES approver_groups(id) ON DELETE SET NULL,
    decided_by        uuid REFERENCES users(id),
    decided_at        timestamptz,
    decision_comment  text,
    version           int NOT NULL DEFAULT 1,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX comp_off_requests_user_idx ON comp_off_requests (user_id, created_at);
CREATE INDEX comp_off_requests_org_status_idx ON comp_off_requests (org_id, status, created_at);

CREATE TABLE comp_off_request_dates (
    id                   uuid PRIMARY KEY,
    comp_off_request_id  uuid NOT NULL REFERENCES comp_off_requests(id) ON DELETE CASCADE,
    org_id               uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id              uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    worked_date          date NOT NULL,
    portion              numeric(2,1) NOT NULL CHECK (portion IN (0.5, 1.0)),
    -- False once the request is rejected or withdrawn: the date can be claimed again.
    active               boolean NOT NULL DEFAULT true
);
-- A date is claimed once across pending and approved requests (FR-031).
CREATE UNIQUE INDEX comp_off_request_dates_claim_idx
    ON comp_off_request_dates (user_id, worked_date) WHERE active;
CREATE INDEX comp_off_request_dates_request_idx ON comp_off_request_dates (comp_off_request_id);

CREATE TABLE comp_off_credits (
    id                        uuid PRIMARY KEY,
    org_id                    uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id                   uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    comp_off_request_date_id  uuid NOT NULL UNIQUE REFERENCES comp_off_request_dates(id),
    worked_date               date NOT NULL,
    days                      numeric(2,1) NOT NULL CHECK (days IN (0.5, 1.0)),
    days_used                 numeric(3,1) NOT NULL DEFAULT 0 CHECK (days_used >= 0),
    -- Held by pending comp-off leave; never expires while held (FR-060).
    days_reserved             numeric(3,1) NOT NULL DEFAULT 0 CHECK (days_reserved >= 0),
    days_expired              numeric(3,1) NOT NULL DEFAULT 0 CHECK (days_expired >= 0),
    -- Expires at the start of this date (org time zone).
    expires_on                date NOT NULL,
    expired_at                timestamptz,
    warned_at                 timestamptz,
    revoked_at                timestamptz,
    revoked_by                uuid REFERENCES users(id),
    revoke_reason             text,
    created_at                timestamptz NOT NULL DEFAULT now(),
    CHECK (days_used + days_reserved + days_expired <= days),
    CHECK ((revoked_at IS NULL) = (revoke_reason IS NULL))
);
CREATE INDEX comp_off_credits_user_idx ON comp_off_credits (user_id, expires_on);

-- Which credits a comp-off leave request holds (pending) or has used (approved).
CREATE TABLE comp_off_usages (
    comp_off_credit_id  uuid NOT NULL REFERENCES comp_off_credits(id) ON DELETE CASCADE,
    leave_request_id    uuid NOT NULL REFERENCES leave_requests(id) ON DELETE CASCADE,
    org_id              uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    days_reserved       numeric(3,1) NOT NULL DEFAULT 0 CHECK (days_reserved >= 0),
    days_used           numeric(3,1) NOT NULL DEFAULT 0 CHECK (days_used >= 0),
    PRIMARY KEY (comp_off_credit_id, leave_request_id)
);
CREATE INDEX comp_off_usages_request_idx ON comp_off_usages (leave_request_id);

ALTER TABLE comp_off_requests ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON comp_off_requests USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE comp_off_request_dates ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON comp_off_request_dates USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE comp_off_credits ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON comp_off_credits USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE comp_off_usages ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON comp_off_usages USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());

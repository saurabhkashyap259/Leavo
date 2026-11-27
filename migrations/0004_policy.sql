-- T034: leave types, restricted-type grants, holidays.

CREATE TYPE leave_availability AS ENUM ('all', 'restricted');
CREATE TYPE holiday_kind AS ENUM ('mandatory', 'optional');

CREATE TABLE leave_types (
    id                              uuid PRIMARY KEY,
    org_id                          uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    code                            text NOT NULL CHECK (code ~ '^[A-Z]{1,6}$'),
    name                            text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 60),
    color                           text NOT NULL CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
    yearly_quota                    numeric(5,1) CHECK (yearly_quota >= 0 AND yearly_quota * 2 = trunc(yearly_quota * 2)),
    pending_yearly_quota            numeric(5,1) CHECK (pending_yearly_quota >= 0 AND pending_yearly_quota * 2 = trunc(pending_yearly_quota * 2)),
    is_comp_off                     boolean NOT NULL DEFAULT false,
    receives_carry_forward          boolean NOT NULL DEFAULT false,
    allow_half_day                  boolean NOT NULL DEFAULT true,
    allow_negative                  boolean NOT NULL DEFAULT false,
    negative_limit                  numeric(5,1) NOT NULL DEFAULT 5 CHECK (negative_limit >= 0),
    max_days_per_request            numeric(5,1) CHECK (max_days_per_request > 0),
    min_notice_days                 int NOT NULL DEFAULT 0 CHECK (min_notice_days >= 0),
    max_backdate_days               int NOT NULL DEFAULT 0 CHECK (max_backdate_days BETWEEN 0 AND 30),
    attachment_required_above_days  numeric(4,1) CHECK (attachment_required_above_days >= 0),
    availability                    leave_availability NOT NULL DEFAULT 'all',
    active                          boolean NOT NULL DEFAULT true,
    sort_order                      int NOT NULL DEFAULT 0,
    created_at                      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, code),
    CHECK (is_comp_off = (yearly_quota IS NULL))
);
CREATE UNIQUE INDEX leave_types_one_comp_off_per_org ON leave_types (org_id) WHERE is_comp_off;

CREATE TABLE leave_type_grants (
    id             uuid PRIMARY KEY,
    org_id         uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    leave_type_id  uuid NOT NULL REFERENCES leave_types(id) ON DELETE CASCADE,
    user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    granted_by     uuid REFERENCES users(id),
    granted_at     timestamptz NOT NULL DEFAULT now(),
    revoked_by     uuid REFERENCES users(id),
    revoked_at     timestamptz
);
CREATE UNIQUE INDEX leave_type_grants_active_idx ON leave_type_grants (leave_type_id, user_id) WHERE revoked_at IS NULL;

CREATE TABLE holidays (
    id          uuid PRIMARY KEY,
    org_id      uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    date        date NOT NULL,
    name        text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    kind        holiday_kind NOT NULL DEFAULT 'mandatory',
    location    text,
    source      text NOT NULL DEFAULT 'manual',
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, date, name)
);
CREATE INDEX holidays_org_date_idx ON holidays (org_id, date);

ALTER TABLE leave_types ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON leave_types USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE leave_type_grants ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON leave_type_grants USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE holidays ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON holidays USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());

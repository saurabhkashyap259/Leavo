-- T032: organizations, domains, users, approver groups, instance settings.

CREATE TYPE user_role AS ENUM ('employee', 'approver', 'admin');
CREATE TYPE user_status AS ENUM ('invited', 'active', 'deactivated');
CREATE TYPE instance_mode AS ENUM ('single', 'multi');

CREATE TABLE organizations (
    id                      uuid PRIMARY KEY,
    name                    text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    timezone                text NOT NULL DEFAULT 'Asia/Kolkata',
    weekend_mask            smallint NOT NULL DEFAULT 96 CHECK (weekend_mask BETWEEN 0 AND 126),
    leave_year_start_month  smallint NOT NULL DEFAULT 1 CHECK (leave_year_start_month BETWEEN 1 AND 12),
    carry_forward_cap       numeric(5,1) NOT NULL DEFAULT 0 CHECK (carry_forward_cap >= 0),
    comp_off_expiry_days    int NOT NULL DEFAULT 90 CHECK (comp_off_expiry_days BETWEEN 1 AND 365),
    reminder_after_hours    int NOT NULL DEFAULT 48 CHECK (reminder_after_hours > 0),
    created_at              timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE org_domains (
    id          uuid PRIMARY KEY,
    org_id      uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    domain      citext NOT NULL UNIQUE CHECK (domain::text = lower(domain::text) AND position('@' IN domain) = 0),
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE approver_groups (
    id          uuid PRIMARY KEY,
    org_id      uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

CREATE TABLE users (
    id                  uuid PRIMARY KEY,
    org_id              uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email               citext NOT NULL UNIQUE,
    full_name           text NOT NULL CHECK (char_length(full_name) BETWEEN 1 AND 120),
    employee_code       text,
    department          text,
    date_of_joining     date,
    role                user_role NOT NULL DEFAULT 'employee',
    status              user_status NOT NULL DEFAULT 'active',
    approver_group_id   uuid REFERENCES approver_groups(id) ON DELETE SET NULL,
    last_notify_list    jsonb NOT NULL DEFAULT '[]',
    anonymised_at       timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, employee_code)
);
CREATE INDEX users_org_idx ON users (org_id);

CREATE TABLE approver_group_members (
    group_id    uuid NOT NULL REFERENCES approver_groups(id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    org_id      uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id)
);

-- Singleton, instance-wide (not tenant data).
CREATE TABLE instance_settings (
    id                              smallint PRIMARY KEY CHECK (id = 1),
    mode                            instance_mode NOT NULL DEFAULT 'single',
    setup_token_hash                bytea,
    setup_token_failed_attempts     int NOT NULL DEFAULT 0,
    setup_token_locked_until        timestamptz,
    setup_completed_at              timestamptz,
    last_scheduler_tick_at          timestamptz,
    last_update_check_at            timestamptz,
    latest_version_seen             text
);
INSERT INTO instance_settings (id) VALUES (1);

-- Row-Level Security (FR-048).
ALTER TABLE organizations ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON organizations USING (id = app_org_id()) WITH CHECK (id = app_org_id());
ALTER TABLE org_domains ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON org_domains USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE approver_groups ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON approver_groups USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE users ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON users USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());
ALTER TABLE approver_group_members ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant ON approver_group_members USING (org_id = app_org_id()) WITH CHECK (org_id = app_org_id());

-- Narrow lookups needed before the tenant is known (sign-in, signup, setup).
-- SECURITY DEFINER runs as the migration owner, which bypasses RLS; each returns minimal data.
CREATE FUNCTION auth_find_user(p_email citext)
    RETURNS TABLE (id uuid, org_id uuid, role user_role, status user_status)
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public
AS $$ SELECT u.id, u.org_id, u.role, u.status FROM users u WHERE u.email = p_email $$;

CREATE FUNCTION auth_find_org_by_domain(p_domain citext)
    RETURNS uuid
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public
AS $$ SELECT d.org_id FROM org_domains d WHERE d.domain = p_domain $$;

CREATE FUNCTION instance_has_org()
    RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public
AS $$ SELECT EXISTS (SELECT 1 FROM organizations) $$;

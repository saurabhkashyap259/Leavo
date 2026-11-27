-- T033: sign-in codes, sessions, rate limits, email action tokens.
-- Not tenant-scoped by RLS: they are read before the tenant is known, only by
-- unguessable keys (code hash, token hash, nonce) or by email + purpose.

CREATE TYPE code_purpose AS ENUM ('login', 'signup', 'register_org', 'setup', 'recovery');

CREATE TABLE login_codes (
    id           uuid PRIMARY KEY,
    email        citext NOT NULL,
    purpose      code_purpose NOT NULL,
    code_hash    bytea NOT NULL,
    payload      jsonb NOT NULL DEFAULT '{}',
    expires_at   timestamptz NOT NULL,
    attempts     smallint NOT NULL DEFAULT 0,
    consumed_at  timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX login_codes_lookup_idx ON login_codes (email, purpose, created_at DESC);

CREATE TABLE sessions (
    id                   uuid PRIMARY KEY,
    user_id              uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    org_id               uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    token_hash           bytea NOT NULL UNIQUE,
    keep_signed_in       boolean NOT NULL DEFAULT false,
    expires_at           timestamptz NOT NULL,
    absolute_expires_at  timestamptz NOT NULL,
    user_agent           text,
    ip                   inet,
    created_at           timestamptz NOT NULL DEFAULT now(),
    last_seen_at         timestamptz NOT NULL DEFAULT now(),
    revoked_at           timestamptz,
    CHECK (expires_at <= absolute_expires_at)
);
CREATE INDEX sessions_user_active_idx ON sessions (user_id) WHERE revoked_at IS NULL;
CREATE INDEX sessions_org_active_idx ON sessions (org_id) WHERE revoked_at IS NULL;

CREATE TABLE rate_limits (
    bucket        text NOT NULL,
    key           text NOT NULL,
    window_start  timestamptz NOT NULL,
    count         int NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket, key, window_start)
);

CREATE TABLE action_tokens (
    id            uuid PRIMARY KEY,           -- the nonce carried in the email link
    org_id        uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    request_id    uuid NOT NULL,
    request_kind  text NOT NULL CHECK (request_kind IN ('leave', 'comp_off', 'cancel')),
    approver_id   uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    decision      text NOT NULL CHECK (decision IN ('approve', 'reject')),
    expires_at    timestamptz NOT NULL,
    used_at       timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now()
);

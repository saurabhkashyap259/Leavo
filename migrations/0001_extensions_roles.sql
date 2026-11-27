-- T031: extensions, the non-owner application role, default privileges, tenant helper.
-- The app connects with DATABASE_URL and switches every connection to `leavo_app`
-- (SET ROLE), so Row-Level Security applies even when DATABASE_URL is a superuser.

CREATE EXTENSION IF NOT EXISTS citext;

DO $$
BEGIN
    CREATE ROLE leavo_app NOLOGIN;
EXCEPTION
    WHEN duplicate_object OR unique_violation THEN NULL; -- role is cluster-wide; may already exist
END
$$;

-- Let the migrating user switch to leavo_app (no-op for superusers).
DO $$
BEGIN
    EXECUTE format('GRANT leavo_app TO %I', current_user);
EXCEPTION
    WHEN others THEN NULL;
END
$$;

GRANT USAGE ON SCHEMA public TO leavo_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO leavo_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT USAGE, SELECT ON SEQUENCES TO leavo_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT EXECUTE ON FUNCTIONS TO leavo_app;

-- Current tenant for RLS policies. NULL (no rows visible) when unset or reset to ''.
CREATE FUNCTION app_org_id() RETURNS uuid
    LANGUAGE sql STABLE
AS $$ SELECT NULLIF(current_setting('app.org_id', true), '')::uuid $$;

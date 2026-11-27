-- T118/T122: approval support.

-- The scheduler runs outside any organization; RLS hides `organizations` from it. This lists
-- only ids, and each job then opens its own org-scoped transaction.
CREATE FUNCTION job_org_ids()
    RETURNS SETOF uuid
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public
AS $$ SELECT id FROM organizations ORDER BY id $$;

-- Action links are looked up by request when a decision marks them all used.
CREATE INDEX action_tokens_request_idx ON action_tokens (request_id);

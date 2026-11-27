-- T207: remember a one-way fingerprint of LEAVO_SECRET_KEY so a restart with a different key
-- can sign everyone out and void unused codes and action links (FR-063). Session tokens are
-- stored as plain SHA-256 digests, so without this a rotated key would leave sessions valid.
ALTER TABLE instance_settings ADD COLUMN secret_fingerprint bytea;

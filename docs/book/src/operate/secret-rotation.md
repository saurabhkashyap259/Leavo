# Rotate the secret key

`LEAVO_SECRET_KEY` signs sign-in codes, sessions and the Approve/Reject links in emails. Rotate
it after a suspected compromise of the server or the `.env` file.

```bash
NEW=$(openssl rand -hex 32)
sed -i "s/^LEAVO_SECRET_KEY=.*/LEAVO_SECRET_KEY=$NEW/" .env
docker compose up -d
```

On start Leavo notices the key changed and:

- signs **everyone** out, on every device;
- voids sign-in and recovery codes that were not used yet;
- voids Approve/Reject links in emails already sent (approvers decide in the app instead).

The log says `LEAVO_SECRET_KEY changed: everyone was signed out`. Leavo stores only a one-way
fingerprint of the key, never the key. Update your backup of `.env` afterwards.

To sign everyone out **without** changing the key (for example after an employee's laptop was
stolen), an admin can use *Sign everyone out* (`POST /api/v1/admin/org/sign-out-all`).

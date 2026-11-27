# Sign in when email is down

Leavo has no passwords: people sign in with a code sent by email. If email stops working, an
operator with access to the server can let an admin in:

```bash
docker compose exec leavo leavo recovery-login admin@example.com
```

It prints a link that:

- works only for an **active Org Admin** (anyone else is refused);
- is valid for **15 minutes** and works **once**;
- starts a normal session and lands on the admin home;
- is recorded in the audit log as `auth.recovery_login` (and `auth.recovery_link` when issued).

Then open **Admin › Instance** to see the last email error and send a test email once the mail
settings are fixed. Changing `SMTP_URL` needs a restart.

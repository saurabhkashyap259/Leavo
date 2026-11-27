# Leavo

Leavo is an open-source leave tracker you run yourself: passwordless sign-in, leave requests
and approvals, balances with a full history, comp-offs, holidays, year-end carry forward and an
import from Keka. It ships as **one Docker image** and needs only **PostgreSQL**.

This guide is for the person who installs and runs Leavo. Start with the
[Quickstart](quickstart.md); the install pages cover specific platforms, and the Operate
section covers upgrades, backups and monitoring.

What you need:

- A server or container platform that can run a Linux image (amd64 or arm64), 1 vCPU and 1 GB
  of memory are enough for a few hundred people.
- PostgreSQL 15 or newer (the bundled Compose file uses PostgreSQL 18).
- An SMTP service for sign-in codes and notifications.
- A domain name with HTTPS in front of Leavo.

# Twitch PostgreSQL DSN Rotation

This rotates only the `TWITCH_ANALYTICS_DSN` PostgreSQL password. It does not
rotate Infisical `ENCRYPTION_KEY`, bot `DB_MASTER_KEY_V1`, or any data
encryption key.

## Manual Rotation

```bash
cd /home/naniadm/Documents/Deadlock-Twitch-Bot
./scripts/rotate_twitch_pg_dsn.py
```

If the runtime Infisical token is read-only, the script prompts for a temporary
write token before changing PostgreSQL.

## Scheduled Rotation

Create a protected env file containing a scoped Infisical write token:

```bash
sudo install -d -m 0750 -o root -g naniadm /etc/deadlock-bots
sudo install -m 0640 -o root -g naniadm /dev/null /etc/deadlock-bots/twitch-pg-rotation.env
sudoedit /etc/deadlock-bots/twitch-pg-rotation.env
```

Content:

```bash
ROTATION_INFISICAL_SERVICE_TOKEN=<temporary-or-scoped-write-token>
```

The token should be scoped to the environment/path that contains
`TWITCH_ANALYTICS_DSN`. Today that is `prod:/`; after the path migration it
should be the Twitch service path only.

Install the user timer:

```bash
mkdir -p ~/.config/systemd/user
cp /home/naniadm/Documents/Deadlock-Twitch-Bot/ops/deadlock-twitch-pg-dsn-rotation.service ~/.config/systemd/user/
cp /home/naniadm/Documents/Deadlock-Twitch-Bot/ops/deadlock-twitch-pg-dsn-rotation.timer ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now deadlock-twitch-pg-dsn-rotation.timer
```

Dry run the preflight:

```bash
/home/naniadm/Documents/Deadlock-Twitch-Bot/scripts/rotate_twitch_pg_dsn_scheduled.sh \
  --preflight-only \
  --skip-function-test
```

Run one real scheduled-style rotation:

```bash
systemctl --user start deadlock-twitch-pg-dsn-rotation.service
```

Check results:

```bash
systemctl --user status deadlock-twitch-pg-dsn-rotation.service --no-pager
tail -n 20 /home/naniadm/Documents/Deadlock-Twitch-Bot/logs/secret_rotation_audit.jsonl
/home/naniadm/Documents/manage-twitch-services.sh status all
```

## Safety Behavior

- Generates a fresh password automatically.
- Updates PostgreSQL first, verifies the new DSN, then writes Infisical.
- Restarts Twitch worker and dashboard.
- Runs DB, internal API, and dashboard health checks.
- Rolls back PostgreSQL and Infisical if an update or health check fails.
- Writes a secret-free JSONL audit log.

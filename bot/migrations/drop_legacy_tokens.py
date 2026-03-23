"""Migration: Legacy-Klartext-Token-Spalten aus twitch_raid_auth entfernen.

Hintergrund:
    Raid-OAuth-Tokens wurden fruher als Klartext in den Spalten
    legacy_access_token, legacy_refresh_token, legacy_scopes, legacy_saved_at
    gespeichert. Seit der AES-256-GCM-Migration werden nur noch die
    *_enc Spalten genutzt. Die Legacy-Spalten koennen sicher entfernt werden.

Ausfuehren:
    python -m bot.migrations.drop_legacy_tokens

Idempotent: Spalten die nicht existieren werden still ignoriert (IF EXISTS).
"""

from __future__ import annotations

import sys

from ..storage import pg as storage_pg


def run() -> None:
    legacy_cols = (
        "legacy_access_token",
        "legacy_refresh_token",
        "legacy_scopes",
        "legacy_saved_at",
    )

    with storage_pg.transaction() as conn:
        for col in legacy_cols:
            conn.execute(
                f"ALTER TABLE twitch_raid_auth DROP COLUMN IF EXISTS {col}"
            )
            print(f"  Dropped column twitch_raid_auth.{col} (or did not exist)")

    print("Migration drop_legacy_tokens: done.")


if __name__ == "__main__":
    run()
    sys.exit(0)

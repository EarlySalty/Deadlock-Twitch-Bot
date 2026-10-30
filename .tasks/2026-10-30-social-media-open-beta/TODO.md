# Social media dashboard open beta

SHA: 0014d11d8 (main), all commits merged after gate ALLOW.

## Function proof after deploy
- `deploy-twitch-release --pruefen` exits 0 on the new SHA.
- `journalctl -p err` for the dashboard unit shows nothing new since the restart.
- `https://deutsche-deadlock-community.de/twitch/social-media` answers without 5xx.
- Served SocialMediaManager bundle contains `social-media-early-beta-banner-dismissed`.
- `social_media_partner_access` gets a row with `granted_by = 'open_beta'` at the first partner visit.

## DMs
Do NOT send before the user approved the text in `DM-DRAFT.md`. Recipients: active partners, Discord linked, no access row, not in core.dm_optout. Rebuild the list directly before sending. Send via broker send-DM with `DmPurpose::Proactive`, so the global opt-out gate applies.

## Proof state (2026-10-30, release 0014d11d8)
- `--pruefen` exit 0, journal err empty since restart, page 303 to login (no 5xx), served bundle SocialMediaManager-BCzdnxbe.js contains the banner key.
- Open: no `open_beta` row yet (13 rows: 12 system, 1 earlysalty). Confirm after the first partner visit, at the latest after the DMs.
- DMs not sent, waiting for the user's approval.

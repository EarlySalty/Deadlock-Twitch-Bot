# Register

Intent-Thread-ID: de7d1570-c98a-44a8-8dcf-e161568e5020; Codex-Hauptsession /root.

| Paket | Thread-ID | Modell | Status | Worktree | Letzte Meldung |
| --- | --- | --- | --- | --- | --- |
| A: Timer-Grenzen | e9305f57-4f63-4975-bbd5-6e42cb70c2ba | Opus 4.8 | umgesetzt, Checks gruen, wartet auf Abnahme | /home/nathanael/.worktrees/tb-promo-admin-grenzen | Rebaset auf b440e84a; HEAD ce9e7450; Fixes + P2-Reset behoben |

## Commits (Branch fix/promo-admin-grenzen, Basis b440e84a)

- 86c1b1d3 fix(promo): Chat-Grenzen auch fuer Community-Timer und Viewer-Spike
- ce9e7450 fix(promo): neue Chatter beim Versand als gesehen zaehlen (Bucket UNION Viewer)

Abnahme und Deployment liegen bei der Hauptsession. Kein Merge, kein Deploy durch den Worker.
Basis b440e84a bleibt fix (Peer-Overlay wartet auf unseren Abschluss).

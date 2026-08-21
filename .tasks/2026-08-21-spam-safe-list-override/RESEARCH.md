status: aktiv
datum: 2026-08-21
klasse: mittel

## Auftrag

Das Modell trifft im Twitch-Spam-Verdachtspfad die Entscheidung und lernt ein
Spam-Muster automatisch. Der Discord-Button ist nur der manuelle Gegen-Override:

- AI `harmlos` → `Als Spam korrigieren` und als Spam lernen.
- AI `Spam` → `Als harmlos korrigieren`, das falsche Spam-Muster entfernen und die
  vollständige Originalnachricht als Safe-Pattern lernen.

Ein aktives Safe-Pattern darf nur bei einer solchen menschlichen Korrektur
entstehen. Es wird vor Score-Berechnung und vor jedem LLM-Aufruf geprüft.

## Bestand (belegt)

- `tb-chat::SpamAiReviewer` lernt bei `is_spam=true` über
  `twitch_auto_learned_spam_patterns`; `source_message` enthält die Nachricht
  bis 500 Zeichen und die Discord-Korrektur referenziert die Spam-Row-ID.
- `pipeline.rs` baut das Discord-`spam_learning`-Payload und ruft den Judge nur
  nach dem normalen Spam-Score auf.
- `dl-changelog` wählt derzeit abhängig vom Urteil einen Button; der Safe-Feedback-
  Pfad aus dem vorherigen Teilauftrag war eine Bestätigung statt Gegen-Override.
- `dl-bridges` sendet `correct`-/`learn`-Klicks an die interne Twitch-API und
  deaktiviert den Button nach Erfolg.
- `twitch_auto_learned_safe_patterns` existiert bereits in der Migration
  `20260630141000_chat_moderation_runtime_tables.sql`.
- `chat_wiring.rs` lädt gelernte Muster periodisch neu; der bestehende Reload-Takt
  beträgt 120 Sekunden.

## Festgelegte Semantik

- Safe-Match ist ein exakter Volltextvergleich nach derselben sicheren
  Kanonisierung wie der Spam-Filter: NFKC/Homoglyphen, Whitespace-Kompression,
  Trim und Kleinschreibung. Teilstrings und längere Nachrichten matchen nicht.
- Safe-Patterns aus der aktiven Tabelle werden nur mit
  `source_channel = 'discord-correction'` geladen. Damit werden alte archivierte
  bzw. AI-erzeugte Safe-Zeilen nicht rückwirkend aktiv.
- Safe-Treffer überspringen im Spam-Pfad Score, Moderationsaktion und den
  Spam-Judge vollständig; sie werden als `SAFE_PATTERN` im Review-Log sichtbar
  gemacht.
- Der manuelle Safe-Write und das Entfernen des falschen Spam-Musters erfolgen in
  einer Transaktion. Die Clean-Review-Zeile bleibt als Audit erhalten.

## Risiken

- Ein Safe-Override ist eine privilegierte Lernaktion und bleibt auf den bestehenden
  internen Auth-/Moderationspfad beschränkt.
- Der Cache-Reload ist nicht synchron zum Klick; ein neuer Safe-Eintrag greift ohne
  Neustart spätestens beim vorhandenen 120-Sekunden-Reload.
- Discord-custom_id darf nicht die gesamte Nachricht enthalten. Bei AI-Spam wird
  deshalb die bestehende gelernte Spam-Row-ID als stabiler Kurz-Handle genutzt; die
  API holt die Originalnachricht serverseitig.

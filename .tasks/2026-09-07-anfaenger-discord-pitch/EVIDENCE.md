# Anfänger gehören in den Discord-Anlass

## Ursache live gemessen

Systemjournal `deadlock-twitch-bot-rust`, 2026-09-07 12:15:00 UTC (14:15 Ortszeit), Kanal coolysdl, Chatter chrisqlso: Nachricht über erste MOBA-Erfahrung führte zu `verdict=unsure`, `confidence=0.6`, `is_newcomer=true`, `has_strong_access=false`, `action=ask_confirmation`.
Der separate Zugangs-Responder merkte Begeisterung zehn Minuten lang und machte danach normale Frage- oder Spielwörter zu Zugangskandidaten. Der Judge sah nur die aktuelle Nachricht; Unsicherheit reichte bei Neulingen für die feste Spiel-Invite-Rückfrage.

## Änderung

Interessefenster und schwache Spielsignale entfernt. Ein ausdrücklich genanntes Zugangswort plus Frage/Zugangsmangel ist vor dem Judge erforderlich. Anfänger, Hero-Suche und MOBA-Erfahrung erzeugen dadurch weder Judge-Aufruf noch Zugangsrückfrage. Der bestehende Anlass-Prompt ordnet diese Aussagen new_player zu, bietet weich unseren Discord zum Fragen und gemeinsamen Spielen an und schließt echte Spielzugangsfragen aus. Neulingsregister, Limits, Link- und JoinPhrase-Filter bleiben unverändert.

## Prüfung

- 34 Invite-Responder-Tests grün, einschließlich vollständiger Screenshot-Sequenz und explizitem Deadlock-Invite.
- 29 Anlass-Pitch-Tests grün, einschließlich weichem Discord-Angebot durch vorhandene Filter.
- cargo check tb-chat durch unabhängigen Rust-Reviewer grün.
- Beide geänderten Dateien mit rustfmt geprüft. Workspace-fmt ist vorbestehend breit rot.
- Vorgeschriebener Aufruf gate_hook.py --review versucht; Datei fehlt unter dem dokumentierten Pfad. Tatsächlicher Pre-Push-Hook ist .githooks/pre-push mit security-scan-local.sh. Unabhängiger Quality-Review ergänzt die Prüfung.
- Unabhängiger Quality-Agent: keine BLOCKINGs, fachliche Freigabe. Die Pipeline betreibt Anlass- und Zugangsantwort bereits unabhängig; die Prompt-Grenze ist keine neue globale Garantie gegen jeden bestehenden gezielt-Fallback.

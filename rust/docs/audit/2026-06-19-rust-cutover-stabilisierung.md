# Rust-Cutover-Stabilisierung vom 19.06.2026

Grundlage ist die Grillme-Entscheidungsliste vom 15.06.2026. Diese Änderung
schließt die im laufenden Rust-Betrieb gefundenen Abweichungen.

## Bewusst deaktivierte Funktionen

Folgende Worker starten standardmäßig nicht:

- Highlight-Erstellung
- Twitch-Clip-Abruf für die Social-Media-Pipeline
- Stream-Audio-Capture und Transkription

Die Implementierungen bleiben repariert und testbar. Eine Aktivierung ist nur
über das jeweilige explizite Opt-in mit dem Wert `1` möglich:

- `TB_HIGHLIGHT_CLIPPER_ENABLED`
- `TB_CLIP_FETCHER_ENABLED`
- `ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED`

## Behobene Laufzeitfehler

- EventSub-Telemetrie schreibt Geschenk- und Automatikmerkmale als Boolean.
- Clip-Zeitstempel und Zähler entsprechen den produktiven PostgreSQL-Typen.
- Die Schema-Migration vereinheitlicht bereits vorhandene Altspalten.
- Dead-Letter-Requeue plant den konkreten Eintrag wirklich neu ein.
- Observability und Chatter-Diagnose lesen persistenten Zustand statt
  Platzhalterantworten zu liefern.
- Dauerhaft blockierte Chat-Abonnements werden nicht mehr als wiederkehrender
  Reconcile-Fehler gewertet.
- Prozentmetriken verwenden den bisherigen Peak als Bezugsgröße und erzeugen
  dadurch keine Warnschleife oberhalb von 100 Prozent.
- Vorhandene Discord-Live-Rollen werden wiederverwendet. Unterstützt der aktive
  Broker keine Rollenerstellung, wird nach dem ersten 404 kein weiterer
  Anlageversuch in derselben Laufzeit erzeugt.

## Betriebsregel

Die drei deaktivierten Worker dürfen erst nach einer neuen, dokumentierten
Entscheidung und einem kontrollierten Testlauf aktiviert werden. Ein vorhandener
Codepfad oder eine erfolgreiche Reparatur gilt nicht als Freigabe.

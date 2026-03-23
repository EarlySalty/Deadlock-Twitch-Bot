# PROJ-1: Vorgecachte Partner-Raid-Scores mit Eventgetriebenem Refresh

## Status: 🔵 Done

## Summary
- Die Partner-Raid-Auswahl wird von einer On-Demand-Berechnung auf einen persistenten Score-Cache umgestellt.
- Der Auto-Raid liest beim Offline-Event nur noch live verfügbare Partner und deren fertige Cache-Scores.
- Der periodische Refresh alle 5 Minuten bleibt bestehen, ist aber nur ein Reconciliation-Sicherheitsnetz.
- Score-relevante Änderungen müssen eventgetrieben sofort in den Cache einfließen und dürfen nicht bis zum nächsten 5-Minuten-Lauf warten.

## Ziele
- Die Raid-Auswahl im Offline-Pfad deterministisch und schnell machen.
- Historische Berechnung vollständig aus dem Auswahlpfad entfernen.
- Neue Partner bis zum 10. erhaltenen erfolgreichen Incoming-Raid sichtbar bevorzugen.
- Boost-Partner proportional bevorzugen, ohne die Grundlogik additiv zu verzerren.
- Live-, Raid- und Plan-Änderungen direkt in den Cache übernehmen.

## Nicht-Ziele
- Keine Änderung an öffentlichen APIs.
- Keine Änderung am Non-Partner-Fallback.
- Keine Änderung an Blacklist, Cooldown, Eligibility-Filtern oder an der Raid-Ausführung selbst.
- Keine Einführung eines separaten Score-Modells für Non-Partner.

## User Stories
- Als Auto-Raid-System möchte ich fertige Partner-Scores aus einem Cache lesen, damit die Zielauswahl beim Offline-Event keine Zeit mit Historienberechnung verliert.
- Als neu integrierter Partner möchte ich in meinen ersten erhaltenen Netzwerk-Raids bevorzugt werden, damit ich schneller Sichtbarkeit im Netzwerk bekomme.
- Als Boost-Partner möchte ich einen proportionalen Score-Vorteil erhalten, damit mein aktiver Raid-Boost messbar in die Auswahl einfließt.
- Als Betreiber möchte ich, dass Live-Starts, Live-Enden, erfolgreiche Incoming-Raids und Plan-Änderungen sofort im Cache sichtbar werden, damit die Auswahl mit aktuellen Daten arbeitet.
- Als Entwickler möchte ich einen einzigen Postgres-Refresh-Pfad verwenden, damit Logik, Tests und Verhalten konsistent bleiben.

## Acceptance Criteria

### 1. Persistenter Score-Cache
- Es gibt eine neue persistente Tabelle `twitch_partner_raid_scores`, keyed by `twitch_user_id`.
- Pro Partner enthält der Cache mindestens:
  - statische Grundlagen: `avg_duration_sec`, `time_pattern_score_base`, `received_successful_raids_total`, `is_new_partner_preferred`
  - Boost-Daten: `raid_boost_multiplier`
  - aktuelle Live-Daten: `is_live`, `current_started_at`, `current_uptime_sec`
  - fertige Scores: `duration_score`, `time_pattern_score`, `base_score`, `final_score`
  - Tiebreak-/Metadaten: `today_received_raids`, `last_computed_at`
- Offline-Partner bleiben im Cache erhalten.
- Offline-Partner haben `is_live = 0` und werden von der Raid-Auswahl ignoriert, auch wenn ein älterer Score-Snapshot vorhanden ist.

### 2. Einheitlicher Refresh-Pfad
- Es existiert genau ein interner Refresh-Pfad für Partner-Scores, der den Postgres-Produktionspfad abbildet.
- Der Refresh-Pfad kann einen einzelnen Partner und alle aktiven Partner aktualisieren.
- Der Refresh-Pfad lädt:
  - aktive Partner aus `twitch_streamers_partner_state`
  - aktuellen Live-State aus `twitch_live_state`
  - Session-Historie aus `twitch_stream_sessions`
  - Lifetime- und Today-Incoming-Raids aus `twitch_raid_history`
  - `raid_boost_enabled` aus der bestehenden Plan-/Billing-Quelle
- Der Refresh-Pfad schreibt vollständige, fertige Cache-Zeilen zurück in `twitch_partner_raid_scores`.

### 3. Eventgetriebene Aktualisierung
- Der 5-Minuten-Refresh bleibt aktiv, dient aber nur als periodische Reconciliation.
- Jede Änderung, die Score-Inputs verändert, muss zusätzlich einen sofortigen Refresh für den betroffenen Partner auslösen.
- Relevante Sofort-Trigger sind mindestens:
  - `stream.online`
  - `stream.offline`
  - erfolgreicher eingehender Netzwerk-Raid auf einen Partner (`channel.raid` bzw. bestätigte erfolgreiche Raid-Ankunft)
  - Änderung des Partner-Status oder der Raid-Berechtigung, wenn dadurch ein Partner in die Auswahl eintritt oder aus ihr herausfällt
  - Änderung von `raid_boost_enabled` bzw. einer effektiven Plan-Zuordnung
- Nicht score-relevante Events müssen keinen Refresh auslösen.
- Ein relevanter Event darf nicht nur markiert und bis zum nächsten 5-Minuten-Lauf liegen gelassen werden; der Refresh muss sofort im Event-Pfad oder in einem unmittelbar angestoßenen Folgejob ausgeführt werden.
- Wenn ein Event-Refresh fehlschlägt, bleibt der letzte gültige Cache-Eintrag erhalten und der Fehler wird geloggt; der periodische Refresh reconciliert später.

### 4. Scoring-Modell
- Lookback für die Historie ist 45 Tage.
- `duration_score` wird berechnet als `clamp((avg_duration_sec - current_uptime_sec) / avg_duration_sec, 0, 1)`.
- `time_pattern_score` ist der Anteil historischer Sessions, die im gleichen Wochentag-und-Stunde-Bucket wie der aktuelle Bewertungszeitpunkt gestartet sind.
- Die Bucket-Bildung nutzt die Zeitzone `Europe/Berlin`.
- `base_score = 0.5 * duration_score + 0.5 * time_pattern_score`.
- Ein Partner gilt bis einschließlich 9 erhaltene erfolgreiche Incoming-Raids als neu.
- Ab dem 10. erhaltenen erfolgreichen Incoming-Raid entfällt der Newcomer-Vorteil vollständig.
- `new_partner_multiplier` ist standardmäßig linear von `1.25` bei `0` erhaltenen erfolgreichen Incoming-Raids auf `1.0` bei `10+`.
- `raid_boost_multiplier = 1.5` für Boost-Partner und `1.0` für normale Partner.
- `final_score = base_score * new_partner_multiplier * raid_boost_multiplier`.
- Bei weniger als 3 verwertbaren Sessions wird der jeweils unzuverlässige Teil-Score neutral mit `0.5` behandelt.
- Wenn beide Teil-Scores aufgrund fehlender Historie unzuverlässig sind, ergibt sich dadurch ein neutraler `base_score` von `0.5`.

### 5. Auswahl im Offline-Pfad
- `bot/raid/bot.py` nutzt für Partner-Raids nur noch:
  - live verfügbare Partner aus `online_partners`
  - deren fertige Cache-Zeilen aus `twitch_partner_raid_scores`
- Im Auswahlpfad werden keine historischen Sessions, Incoming-Raid-Aggregate oder Multiplikatoren mehr on demand berechnet.
- Hard-Filter bleiben unverändert und werden vor der Score-Auswahl angewendet:
  - Blacklist
  - Cooldown
  - `raid_enabled`
  - bestehende Eligibility-Regeln
- Unter den gefilterten Partnern gewinnt der höchste `final_score`.
- Wenn `abs(final_score_diff) <= 0.05`, gewinnt der Partner mit weniger erfolgreichen erhaltenen Raids am heutigen Berlin-Kalendertag (`today_received_raids`).
- Danach bleibt der bestehende deterministische Fallback erhalten: `viewer_count`, `followers_total`, `started_at`.
- Wenn für einen live verfügbaren Partner kein Cache-Eintrag vorhanden ist, wird im Auswahlpfad keine Inline-Berechnung nachgeholt; der Partner wird für diese Auswahl übersprungen und der Cache-Miss wird geloggt.
- Wenn nach allen harten Filtern und Cache-Prüfungen kein nutzbarer Partner bleibt, bleibt der bestehende Non-Partner-Fallback unverändert aktiv.

### 6. Logging und Nachvollziehbarkeit
- Für jeden Refresh werden mindestens folgende Werte nachvollziehbar loggbar gemacht:
  - `duration_score`
  - `time_pattern_score`
  - `new_partner_multiplier`
  - `raid_boost_multiplier`
  - `final_score`
  - `today_received_raids`
- Für jede Partner-Auswahl wird geloggt:
  - ausgewählter Partner
  - `final_score`
  - ob die Auswahl direkt per Score oder per Tie-Break entschieden wurde
  - der verwendete Tie-Break-Grund, falls vorhanden
- Fehler bei Event-Refreshes und Cache-Misses werden separat geloggt.

### 7. Testabdeckung
- Ein Cache-Refresh erzeugt für einen Offline-Partner einen gültigen Datensatz mit `is_live = 0`.
- Ein `stream.online`-Event aktualisiert den Cache des Partners sofort und setzt die Live-Daten ohne Warten auf den nächsten 5-Minuten-Lauf.
- Ein `stream.offline`-Event setzt `is_live = 0`, ohne dass der Cache-Eintrag verloren geht.
- Ein erfolgreicher eingehender Netzwerk-Raid aktualisiert `received_successful_raids_total`, `today_received_raids` und gegebenenfalls den Newcomer-Status sofort.
- Eine Änderung von `raid_boost_enabled` aktualisiert `raid_boost_multiplier` ohne Warten auf den periodischen Refresh.
- Die Partner-Auswahl nutzt ausschließlich Cache-Daten und führt keine Historienberechnung mehr im Auswahlpfad aus.
- Ein höherer `duration_score` gewinnt bei sonst gleichen Werten.
- Ein höherer `time_pattern_score` gewinnt bei sonst gleichen Werten.
- Ein Boost-Partner mit identischem `base_score` gewinnt durch den `1.5`-Multiplikator gegen einen normalen Partner.
- Neue Partner werden bis einschließlich des 9. erhaltenen erfolgreichen Incoming-Raids bevorzugt; ab dem 10. entfällt der Bonus.
- Bei ähnlichem `final_score` entscheidet `today_received_raids`.
- Cooldown, Blacklist und `raid_enabled` bleiben harte Filter vor der Score-Auswahl.
- Der Non-Partner-Fallback bleibt unverändert.

## Edge Cases
- Ein Partner geht live und ein anderer Stream geht kurz danach offline: Der Live-Partner muss nach dem `stream.online`-Event bereits einen verwertbaren Cache-Eintrag haben oder im aktuellen Auswahlvorgang als Cache-Miss sauber übersprungen werden; es darf keine Inline-Historienberechnung geben.
- Ein Event wird verspätet zugestellt oder verpasst: Der letzte gültige Cache-Eintrag bleibt nutzbar, und der 5-Minuten-Refresh stellt die Konsistenz wieder her.
- Mehrere relevante Events für denselben Partner treffen kurz hintereinander ein: Der Refresh-Pfad muss idempotent sein, sodass am Ende der neueste Zustand im Cache landet.
- Ein Partner hat weniger als 3 verwertbare Sessions für nur einen Teil-Score: Nur dieser Teil wird neutral auf `0.5` gesetzt; der andere Teil darf weiter aus Historie berechnet werden.
- Berlin-Tagesgrenze oder DST-Wechsel: `today_received_raids` und Zeit-Buckets müssen strikt nach `Europe/Berlin` berechnet werden.
- Ein Partner verliert mitten im Live-Betrieb den Boost oder erhält ihn neu: Der Multiplikator muss nach der Plan-Änderung sofort wirksam werden.
- Ein Partner ist im Cache vorhanden, aber aktuell nicht mehr raid-berechtigt: Harte Eligibility-Filter müssen ihn trotzdem vor der Auswahl ausschließen.

## Annahmen
- Der 10. Raid meint erhaltene erfolgreiche eingehende Netzwerk-Raids des Zielpartners in `twitch_raid_history`.
- Der Raid-Boost verstärkt den bestehenden Score proportional und nicht additiv.
- Der Newcomer-Vorteil soll spürbar sein, aber schwächer als ein voller Raid-Boost.
- `stream.online`, `stream.offline` und erfolgreiche `channel.raid`-Folgen sind die primären Echtzeit-Signale; der 5-Minuten-Refresh ist nur ein Sicherheitsnetz für Drift, Event-Verlust und verpasste Änderungen.

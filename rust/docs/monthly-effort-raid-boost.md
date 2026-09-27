# Monatsabschluss und Effort-Raid-Boost

## Fachlicher Vertrag

Der Bot schließt am ersten Kalendertag um 00:05 Uhr in `Europe/Berlin` den
vorherigen Monat. Die Rangfolge verwendet ausschließlich das bestehende
`partner_effort_events`-Ledger der Effort-Engine: Punkte absteigend,
qualifizierte Einladungen absteigend, Zeitpunkt des erstmals erreichten
Endscores aufsteigend. Bei vollständiger Gleichheit entscheidet zuletzt die
stabile Twitch-User-ID, damit Wiederholungen dieselbe Reihenfolge liefern.
Zuschauerzahlen sind kein Bestandteil dieser Rangfolge.

Alle zu diesem Zeitpunkt aktiven Partner erhalten einen permanenten Eintrag
mit Rang, Punkten, Zahl qualifizierter Einladungen und Score-Zeitpunkt.
Saisonabschluss, Ergebnisse und Grant werden in einer gemeinsamen
Datenbanktransaktion geschrieben. Ein eindeutiger Saison-Schlüssel verhindert
Doppelvergaben auch bei konkurrierenden Bot-Instanzen. Die Ergebnis- und
Abschlusstabellen weisen UPDATE, DELETE und TRUNCATE per Trigger zurück.

Die gespeicherte Nummer 1 erhält zwei Boost-Streams, ohne zusätzliche
Mindestpunktzahl. Eine Saison ohne aktive Partner wird ebenfalls geschlossen,
hat aber keinen Gewinner und keinen Grant.

## Stream-Lebenszyklus

Ein Grant verwendet `RAID_BOOST_MULTIPLIER` aus `tb-raid::scoring` (1,15) und
endet für neue Streams genau 30 × 24 Stunden nach seiner Vergabe. Eine
Stream-Reservierung setzt voraus, dass der Grant beim Start des Streams
bereits bestand und noch nicht abgelaufen war. Ein zum Vergabezeitpunkt schon
laufender Stream erhält deshalb keinen nachträglichen Boost.

Der Boost gilt ab Beginn eines berechtigten Streams bis zu dessen Ende. Er
bleibt auch dann bis zum Ende aktiv, wenn die Ablaufzeit während dieses
Streams erreicht wird. Ein Prozessneustart ändert das nicht, weil die
Reservierung in der Datenbank liegt.

Erst beim Abschluss eines Streams werden die bekannten Deadlock-Abschnitte
summiert. Startkategorie und `twitch_channel_updates` bilden die Zeitachse;
unbekannte Kategorien zählen nicht als Deadlock. Kategorieänderungen exakt
am Start werden berücksichtigt. Teilsekunden werden erst nach der Summierung
abgerundet. Ab 1.800 Sekunden wird genau ein Stream verbraucht, darunter keiner.
Zwei qualifizierende Streams erschöpfen den Grant.

Die Zuordnung erfolgt ausschließlich über Twitch-User-ID und Session-ID,
nicht über einen wiederverwendbaren Login. Ein partnerbezogener
Transaktions-Lock serialisiert Abschluss und Reservierung. Noch offene
Reservierungen belegen ihren Platz, auch wenn der Session-Abschluss verzögert
ist. Mehrere überlappende Grants werden nicht multipliziert: Der am frühesten
ablaufende verfügbare Grant wird zuerst verwendet.

## Plan-Boost und Score

**Bewusste Entscheidung: Plan-Boost und Monatsboost werden per logischem ODER
zusammengeführt.** Auch wenn `raid.priority` und ein Monatsgrant gleichzeitig
aktiv sind, beträgt der Multiplikator nur 1,15, niemals 1,3225. Ein
qualifizierender Stream verbraucht den Monatsgrant trotzdem. Er wird nicht
still auf eine spätere planfreie Zeit verschoben.

Der Faktor wirkt nur auf den eigenen Raid-Zielscore, also darauf, wie der
Partner als Ziel eingehender Raids gewichtet wird. Der ausgehende
Raid-Auswahlpfad des Partners wird nicht verändert. Es gibt keine Raid-Garantie.

Nach der Grant-Erzeugung ruft der Scheduler sofort den vorhandenen
`ScoreRefreshResolver` auf. Auch bei einem bereits abgeschlossenen Monat wird
dieser Schreibschritt erneut ausgeführt: Ein Abbruch zwischen Grant-Commit
und Score-Schreiben darf den Refresh nicht dauerhaft verlieren. Fehler und
null geschriebene Score-Zeilen führen zum Retry.

Die vorhandenen Online-/Offline- und Poll-Scorepfade gleichen Stream-Reservierungen
und Verbrauch vor dem Schreiben des Scores ab. Bei Offline-Caches bleiben
die historischen Basiskomponenten erhalten, ein geänderter Boost-Faktor wird
aber im Endscore berücksichtigt. Ein verbrauchter Boost kann deshalb nicht
als alter Endscore im Cache verbleiben.

Der Compute-only-/Vergleichspfad liest ausschließlich bereits persistierte
Stream-Reservierungen. Er schreibt keine Grants oder Verbrauchseinträge und
funktioniert auch mit einer schreibgeschützten Datenbankverbindung. Nur der
echte Score-Refresh gleicht den Stream-Lebenszyklus ab.

## Abhängigkeiten und Sichtbarkeit

Die Migration `20260926230500_monthly_effort_raid_boost.sql` erzeugt nur
Saison-Snapshots und die Grant-/Verbrauchsdaten. Sie legt keine zweite
Effort-Engine und kein zweites Event-Ledger an. Der gelesene Quellvertrag ist:

```text
partner_effort_events(
  partner_twitch_user_id, partner_login, event_type,
  source_id, points, occurred_at
)
```

Die Effort-Engine aus Prompt 2 muss vor dem ersten gewünschten Abschluss
diesen Vertrag bereitstellen. Fehlt die Quelltabelle, wird der Monat nicht
voreilig eingefroren. Der Scheduler wiederholt den Versuch am ersten Tag
alle fünf Minuten; bei einem Neustart an diesem Tag nach 00:05 holt er den
Abschluss nach. Es gibt bewusst keinen automatischen historischen Backfill
bei einer erstmaligen Aktivierung mitten im Monat.

Gewinner und Grant-Status werden für das Dashboard aus Prompt 3 persistiert.
`twitchdash` bekommt dafür nur Leserechte. Dieser PR fügt keine öffentliche
Route, keinen Chat-Befehl und keine Discord- oder Chat-Ankündigung hinzu.
Die Dashboard-Oberfläche selbst gehört zu Prompt 3.

## Prüfungen

`tb-raid` enthält Logiktests für Laufzeit, Berlin-Kalender, Kategoriezeiten und
Nicht-Stapelung sowie PostgreSQL-Tests für den vollständigen Lebenszyklus.
Die Datenbanktests wenden die tatsächliche ausgelieferte Migration in
isolierten Test-Schemas an. Sie prüfen insbesondere parallele Abschlüsse,
unveränderliche Ergebnisse, Stream-Verbrauch, Ablauf während eines Streams,
Identitätstrennung und belegte Reservierungen.

Die bestehenden Rust-CI-Gates führen die featurebezogenen Tests mit einer
expliziten Wegwerf-Testdatenbank aus. Es werden keine produktiven Migrationen
oder Dienste für diese Prüfungen verwendet.

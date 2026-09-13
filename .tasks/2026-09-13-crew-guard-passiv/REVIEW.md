# Auftrag und Antworten auf die Gate-Befunde

Diese Notiz dokumentiert den Nutzerauftrag und die Korrekturen. Sie ersetzt kein Gate-Urteil und weist keine Freigabe an.

Der Nutzer präzisierte: „also Crew Guard da AI Raus und die andern Scam und Spam detections so anpassen das die MIT AI funktionieren aber nicht dauerhaft wieder die ganzen leute erneut Analysiert die da rein schreiben und die man kennt nur weil wir kein gedächtnis haben“. Den bestehenden unbekannten Pfad bestätigte er: „unbekannte konten die irgendeine kacke reinschreiben die über unseren Scam / Spam Pfad erkannt wird ist eigentlich bisher zu 99% korrekt und können wir beibehalten“.

Seine bereitgestellte AGENTS.md bestimmt wörtlich:

> Vertrauen in Chatter netzwerkweit je Twitch-User-ID über alle Partnerkanäle bewerten (Sessions, Nachrichten, Partner- und Ex-Partner-Status, Betreiber, Discord-Verknüpfung), nicht je Kanal; vertraute Konten überspringen Regel-Löschung, Kontoalter-Prüfung und Deepseek-Judge, ein Spam-Urteil setzt das Vertrauen zurück.

Der Bypass bekannter unauffälliger Konten vor Regeln, Kontoalter und KI ist damit die ausdrücklich beauftragte Produktregel. Der erste Gate-Befund würde diesen Auftrag verändern und wird deshalb nicht als Codeänderung übernommen. Qualifikation bleibt ID-basiert und erfordert belastbare verteilte Historie oder vorhandene Partner-/Identitätsevidenz; bestätigte Spam-/Scam-/Banbefunde widerrufen sie über vorhandene Schreibpfade. Unbekannte und widerrufene Konten behalten ihre bisherigen Regeln, KI, Schwellen und Sanktionen.

Die beiden technischen Befunde sind behoben:

1. `persist_radar_alert` reserviert den Meldungsplatz und schreibt den Radar-Ledgereintrag in derselben PostgreSQL-Transaktion. Kontoalterabfrage und Discord-Versand erfolgen außerhalb des Locks. Ein echter fehlgeschlagener INSERT rollt Quote und Wiederholungszahl zurück; der Folgeversuch kann erfolgreich speichern. Der entsprechende PostgreSQL-Test ist bestanden.
2. `crew_archive` führt ausschließlich die bisherige Archivpflege fort: Startlauf und danach täglich, höchstens 50 Discordkartengruppen pro Lauf, unveränderte vorhandene Ablaufzeitpunkte und Claim-Prüfungen. Erfolgreich gelöschte Karten werden samt abgelaufenen Events entfernt; bei fehlenden Löschrechten werden Inhalte wie zuvor redigiert und der spätere Löschversuch bleibt möglich. Keine Crew-KI, Transkription oder Review-Trigger werden wieder gestartet.

Es gibt keine einmalige Massenlöschung und keine Änderung der bisherigen Sechsmonatsfrist. Die reine Archivpflege wird mit isoliertem PostgreSQL und lokalem Discord-Broker-Doppel geprüft.

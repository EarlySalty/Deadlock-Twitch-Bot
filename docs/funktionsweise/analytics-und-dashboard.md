# Statistiken & Streamer-Dashboard

## Worum es geht

Jeder freigeschaltete Streamer-Partner bekommt ein eigenes Dashboard, in dem der Bot alles auswertet, was er während der Streams gesammelt hat — Zuschauerzahlen, Chat-Aktivität, Wachstum, Publikum, Raids und mehr. Das Dashboard übersetzt diese Rohdaten in verständliche Kennzahlen, Diagramme und konkrete Handlungsempfehlungen. Daneben ist es die zentrale Stelle, an der Streamer ihre Bot-Funktionen einstellen (Go-Live-Ankündigung, Abo/Plan, Promo, Verbindungen). Wer das Produkt nur ausprobieren will, kann ein öffentliches Demo-Dashboard mit Beispieldaten ansehen.

## Was der Bot tut

- **Daten im Hintergrund sammeln:** Während ein betreuter Kanal live ist, erfasst der Bot laufend Zuschauerzahlen, Stream-Titel und Kategorie, wer im Chat aktiv ist, neue Follower und Abos. Daraus entsteht über Tage und Wochen eine Statistik-Historie pro Streamer.
- **Kennzahlen berechnen:** Aus den Rohdaten rechnet der Bot übersichtliche Werte aus — z. B. Spitzen- und Durchschnitts-Zuschauer, Wachstum über die Zeit, wie lange Zuschauer im Schnitt bleiben, wie aktiv der Chat ist und welche Tageszeiten am besten laufen.
- **Eine Startseite (Home) zeigen:** Beim Einloggen landet der Streamer auf einer Übersichtsseite mit einem Begrüßungs-/Statusbereich, einem „Health Score" (einer zusammengefassten Gesundheits-Bewertung des Kanals mit Teilwertungen), einer Zusammenfassung des letzten Streams, einem Wochenvergleich der wichtigsten Kennzahlen, Schnellzugriffen und einem Aktionslog des Bots (z. B. ausgeführte Raids, Bans, Service-Warnungen). Dort erscheinen auch Changelog-Einträge, also Hinweise auf neue Funktionen.
- **Ein detailliertes Analyse-Dashboard bereitstellen:** Eine eigene, in Reiter (Tabs) gegliederte Oberfläche zeigt die Auswertungen interaktiv — als Diagramme, Heatmaps, Tabellen und Listen. Themen sind u. a. Übersicht, einzelne Streams im Detail, Sendeplan/beste Sendezeiten, Kategorie-Ranglisten, Chat-Analyse, Wachstum, Publikum, Vergleich mit anderen, Viewer-Verzeichnis, Coaching, Monetarisierung und experimentelle Auswertungen.
- **Coaching-Empfehlungen erstellen:** Der Bot wertet die eigenen Daten des Streamers aus und leitet daraus priorisierte, konkrete Handlungsempfehlungen ab — z. B. zu Sendezeiten, Titeln/Tags, Stream-Länge, Zuschauer-Bindung oder dem Raid-Netzwerk. Diese Empfehlungen rechnet der Bot deterministisch aus den Zahlen aus; es ist keine frei textende KI.
- **Nach dem Stream einen Bericht erstellen:** Nach Stream-Ende baut der Bot eine Zusammenfassung des Streams (Kennzahlen plus Vergleich zu vorherigen Streams). Daraus kann zusätzlich eine in Worte gefasste KI-Auswertung formuliert werden.
- **Konfiguration entgegennehmen:** Über das Dashboard stellt der Streamer seine Bot-Funktionen ein — von der Go-Live-Ankündigung über das Abo bis zu Promo- und Lurker-Steuer-Optionen.

## Wann es passiert

- **Sammeln läuft automatisch**, sobald ein betreuter Kanal live geht — der Streamer muss nichts dafür tun. Es wird laufend und in kurzen Abständen erfasst, solange der Stream läuft.
- **Kennzahlen werden bei Bedarf berechnet**, wenn der Streamer das Dashboard öffnet. Manche Werte der Startseite sind aus Performance-Gründen kurzzeitig zwischengespeichert, sodass nicht jeder Seitenaufruf die komplette Auswertung neu anstößt; der reine Live-Status wird häufiger/aktueller abgefragt.
- **Der Post-Stream-Bericht entsteht nach Stream-Ende**, sobald der Bot die Stream-Sitzung abgeschlossen hat.
- **Die Datenmenge wächst mit der Zeit:** Frisch freigeschaltete Streamer sehen anfangs wenig — viele Auswertungen (Wachstum, Vergleiche, Coaching) werden erst aussagekräftig, wenn über mehrere Streams genug Daten zusammengekommen sind. Solange es zu einem Bereich noch keine Daten gibt, zeigt das Dashboard einen entsprechenden „keine Daten"-Hinweis statt leerer oder erfundener Werte.

## Was Streamer/Viewer sehen

- **Eine Startseite (Home):** Status des eigenen Kanals, Health Score mit Teilwertungen, Zusammenfassung des letzten Streams, Wochenvergleich, Schnellzugriffe und ein Log der letzten Bot-Aktionen. Streamer sehen hier ausschließlich den eigenen Account.
- **Das Analyse-Dashboard mit mehreren Reitern**, je nach Thema. Typische Inhalte:
  - **Übersicht:** zentrale Kennzahlen, Health-Bewertung, eine Liste der Streams, Heatmaps und der Zuschauer-Verlauf.
  - **Einzelne Streams:** eine Stream-Liste, in die man hineinklicken kann, um Zuschauer-Verlauf, Bindung und die aktivsten Chatter eines bestimmten Streams zu sehen.
  - **Sendeplan:** welche Tageszeiten und Wochentage am besten laufen, als Heatmap und mit Timing-Tipps.
  - **Kategorie:** Deadlock-Ranglisten mit Filter, Suche und Sortierung.
  - **Chat-Analyse:** Chat-Kennzahlen, Zuschauer-Treue, aktive Tageszeiten, Profile einzelner Chatter, „Hype-Momente" und behandelte Themen.
  - **Wachstum:** Monats- und Wochenwachstum, Titel- und Tag-Auswertung, wie gut Raids Zuschauer halten.
  - **Publikum:** wie lange zugeschaut wird, der Weg vom Zuschauer zum Follower, Demografie, Lurker-Anteil und welche Titel/Tags besser ziehen.
  - **Vergleich:** Benchmark gegen die Kategorie, Zuschauer-Überschneidung mit anderen Kanälen, geteiltes Publikum.
  - **Viewer-Verzeichnis:** durchsuchbare Liste bekannter Zuschauer, Segmente, Abwanderungs-Risiko und Einzelprofile.
  - **Coaching:** die priorisierten Handlungsempfehlungen.
  - **Monetarisierung:** Werbung, Zuschauer-Einbrüche und Erholung, Bits, Subs, Hype-Train.
- **Diagramme statt nackter Zahlen:** Die Werte werden als interaktive Charts, Heatmaps und Verlaufskurven dargestellt.
- **Gesperrte Inhalte mit Vorschau:** In welchen Reitern und Karten Daten erscheinen, hängt vom Plan ab (siehe unten). Höhere Reiter/Karten sind sichtbar, aber als gesperrt markiert, mit einem Upgrade-Hinweis statt der Daten.
- **Demo-Dashboard:** Eine öffentlich erreichbare Variante zeigt dieselbe Oberfläche mit erfundenen Beispieldaten. Die Demo greift nie auf echte Streamer-Daten zu — sie dient nur zum Anschauen.

## Was Streamer einstellen können

- **Plan/Tier wählen:** Es gibt gestaffelte Pläne (Free, Basic, Erweitert). Je höher der Plan, desto mehr Analyse-Reiter und -Karten sind freigeschaltet. Es gibt eine Vergleichsseite, die zeigt, was welcher Plan enthält, und die zur eigentlichen Abo-/Bezahlseite führt.
- **Abo verwalten:** Plan buchen und bezahlen, Rechnungsdaten hinterlegen, Rechnungen ansehen, das Abo kündigen und das Zahlungs-Portal aufrufen.
- **Go-Live-Ankündigung gestalten:** Aussehen und Text der Discord-Ankündigung (Titel, Beschreibung, Zusatzfelder, Farbe, Bilder, Button, Ping-Rolle) mit Vorschau und Test-Senden. Details dazu im Kapitel zu Go-Live & Ankündigungen.
- **Promo-Nachricht setzen:** Streamer mit passendem Plan können eine eigene Werbe-/Promo-Nachricht hinterlegen, die der Bot im Chat nutzt (mit Pflichtbestandteil und Längenbegrenzung).
- **Lurker-Steuer ein-/ausschalten:** Ein Plan-Feature höherer Tiers, das per Schalter aktiviert wird (und per Chat-Befehl dauerhaft abschaltbar ist).
- **Verbindungen pflegen:** Auf einer Verwaltungsseite sieht der Streamer seinen Twitch- und Discord-Verbindungsstatus, welche Berechtigungen (Scopes) vorhanden oder noch nötig sind, sowie Login, Anzeigename und Account-ID. Fehlen Berechtigungen, ist das die Stelle, um sich neu zu verbinden.
- **Ansicht umschalten:** Im Analyse-Dashboard lässt sich eine Vorschau auf den erweiterten Funktionsumfang einschalten, um zu sehen, was höhere Pläne bieten.

## Grenzen & Sonderfälle

- **Nur der eigene Account:** Ein Streamer sieht in seinen Bereichen ausschließlich die eigenen Daten und kann nur die eigene Konfiguration ändern. Zugriffe auf fremde Accounts werden blockiert. Admins können den Kontext wechseln, normale Streamer nicht.
- **Plan-Grenzen werden serverseitig durchgesetzt:** Die Sperren im Dashboard sind nicht nur Optik — selbst wenn ein gesperrter Bereich in der Oberfläche sichtbar gemacht wird, liefert der Server ohne passenden Plan keine echten Daten dafür.
- **Wenig Daten am Anfang:** Auswertungen, die auf Verlauf oder Vergleich beruhen, brauchen Streams zum Befüllen. „Noch keine Daten" bei einem frischen Account ist normal und kein Fehler.
- **Einige Analysen sind bewusst ausgeblendet:** Bestimmte Auswertungen sind im Code vorhanden, werden im Streamer-Dashboard aber absichtlich nicht angezeigt, weil ihr Nutzen im Alltag zu gering oder zu erklärungsbedürftig ist (z. B. eine Chat-Netzwerk-/Beziehungsansicht und eine spezielle Chat-Reichweiten-Metrik). Das ist eine Produktentscheidung, kein Defekt; sie können später wieder aktiviert werden.
- **Der KI-Analyse-Reiter ist für normale Streamer noch nicht produktiv:** Der entsprechende Reiter kann in der Oberfläche auftauchen, liefert für reguläre Partner aber aktuell keine nutzbaren Daten (derzeit auf Admins beschränkt).
- **Demo ≠ echte Daten:** Das öffentliche Demo-Dashboard zeigt nur Beispieldaten. Werte und Verläufe dort sagen nichts über einen echten Account aus.
- **Coaching ist Statistik, keine KI-Meinung:** Die Coaching-Empfehlungen sind aus den eigenen Zahlen berechnet und damit reproduzierbar. Wo „die KI sagt …" auftaucht, ist das ausschließlich der separate KI-Bereich (z. B. der Post-Stream-Bericht), nicht das Coaching.

## Häufige Fragen

**F: Ich bin gerade freigeschaltet worden, aber mein Dashboard ist fast leer. Ist etwas kaputt?**
A: Nein, das ist normal. Der Bot sammelt die Daten erst, während du live bist, und viele Auswertungen (Wachstum, Vergleiche, Coaching) werden erst nach mehreren Streams aussagekräftig. Bereiche ohne genug Daten zeigen einen „keine Daten"-Hinweis. Mit jedem Stream füllt sich das Dashboard weiter.

**F: Warum sind manche Reiter oder Karten gesperrt?**
A: Welche Analysen du siehst, hängt von deinem Plan ab. Höhere Pläne schalten mehr Reiter und einzelne Karten frei. Gesperrte Inhalte werden dir mit einem Upgrade-Hinweis und einer Vorschau angezeigt, damit du siehst, was sie enthalten. Die Vergleichsseite zeigt, was in welchem Plan steckt.

**F: Was ist der „Health Score"?**
A: Eine zusammengefasste Bewertung der Gesundheit deines Kanals, gebildet aus mehreren Teilwertungen. Er soll dir auf einen Blick zeigen, wo dein Kanal gut dasteht und wo noch Luft ist. Die Teilwertungen siehst du aufgeschlüsselt auf der Startseite.

**F: Sind die Coaching-Tipps von einer KI?**
A: Nein. Die Coaching-Empfehlungen rechnet der Bot direkt aus deinen eigenen Zahlen aus — gleiche Daten ergeben gleiche Empfehlungen. Eine echte KI kommt nur im separaten KI-Bereich zum Einsatz, etwa für die in Worte gefasste Auswertung nach einem Stream.

**F: Was bekomme ich nach einem Stream?**
A: Der Bot fasst den abgeschlossenen Stream zusammen — die wichtigsten Kennzahlen plus Vergleich zu vorherigen Streams. Daraus kann zusätzlich eine narrative KI-Auswertung erstellt werden, die das Ganze in Worten einordnet.

**F: Was zeigt das Demo-Dashboard und kann jemand darüber meine Daten sehen?**
A: Das Demo zeigt die Oberfläche mit reinen Beispieldaten. Es greift nie auf echte Streamer-Accounts zu — über die Demo sind also keine echten Daten von dir oder anderen einsehbar.

**F: Kann ein anderer Streamer meine Statistiken sehen?**
A: Nein. Jeder Streamer sieht in seinen Bereichen nur den eigenen Account; Zugriffe auf fremde Accounts werden blockiert. Lediglich Admins können den Kontext wechseln.

**F: Wo stelle ich meine Bot-Funktionen ein?**
A: Im Dashboard. Die Go-Live-Ankündigung gestaltest du im entsprechenden Builder (mit Vorschau und Test-Senden), Plan und Abo regelst du im Abo-Bereich, Promo-Nachricht und Lurker-Steuer ebenfalls dort, und deine Twitch-/Discord-Verbindungen samt Berechtigungen prüfst du auf der Verwaltungsseite.

**F: Warum sind manche Auswertungen verschwunden oder gar nicht erst da?**
A: Einige Analysen sind bewusst ausgeblendet, weil ihr praktischer Nutzen im Stream-Alltag zu gering oder zu erklärungsbedürftig war. Sie sind nicht gelöscht, sondern können bei Bedarf wieder aktiviert werden. Der KI-Analyse-Reiter ist für normale Streamer aktuell noch nicht produktiv nutzbar.

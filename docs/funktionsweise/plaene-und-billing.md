# Pläne, Features & Abrechnung

## Worum es geht

Der Twitch-Bot ist in der Grundfunktion kostenlos und bringt darüber hinaus bezahlte Pläne mit, die zusätzliche Features freischalten — werbefreier Chat, bevorzugte Raid-Platzierung, das volle Analyse-Dashboard und die Lurker-Steuer-Erinnerung. Bezahlt wird monatlich oder jährlich über das Streamer-Dashboard. Wer den Bot weiterempfiehlt, kann über ein Empfehlungs-Programm (Affiliate) eine Provision auf die Abos der geworbenen Streamer verdienen. Dieses Kapitel beschreibt, was es gibt und was ein Streamer dabei sieht und einstellen kann.

## Was der Bot tut

- **Stellt mehrere Plan-Stufen bereit.** Jeder Streamer hat genau einen aktiven Plan. Welcher Plan aktiv ist, entscheidet, welche Zusatz-Features für seinen Kanal scharf geschaltet sind.
- **Schaltet Features automatisch frei oder sperrt sie.** Die Freischaltung passiert sofort nach Plan-Wechsel, ohne dass der Streamer etwas konfigurieren muss. Features, die nicht im Plan enthalten sind, erscheinen im Dashboard als gesperrte Vorschau-Karte mit Upgrade-Hinweis.
- **Wickelt Zahlung und Abo-Verwaltung über das Dashboard ab.** Bezahlung, Rechnungsübersicht, Profil-Daten (für die Rechnung) und Kündigung laufen alle über die Abo-Seite. Die Zahlung wird über einen externen Zahlungsdienstleister abgewickelt.
- **Hält die kostenlose Basis dauerhaft am Laufen.** Die automatischen Raids und die komplette Auto-Moderation sind nicht an einen bezahlten Plan gebunden — sie laufen auch im kostenlosen Plan immer.
- **Bietet neuen Nutzern eine kostenlose Testphase** mit erweitertem Zugang, damit sie die bezahlten Features ausprobieren können, bevor sie sich entscheiden.
- **Betreibt ein Empfehlungs-Programm.** Wer andere Streamer wirbt und sie dem eigenen Empfehlungs-Konto zuordnet, bekommt eine Provision auf deren bezahlte Abos ausgezahlt.

## Die Plan-Stufen im Überblick

Es gibt drei grobe Stufen — kostenlos, Basis und Erweitert — und mehrere Einzel- bzw. Paket-Pläne darin. Alle Preise sind Netto-Monatspreise; Pakete sind günstiger als die Summe der Einzelpläne.

| Plan | Preis (Netto/Monat) | Was er zusätzlich freischaltet |
|------|---------------------|-------------------------------|
| **Raid Free** (kostenlos) | 0,00 € | Nichts Zusätzliches — die Basis (Auto-Raids, Moderation) läuft ohnehin. |
| **Werbefrei** | 1,99 € | Schaltet die Werbe-Nachrichten des Bots im eigenen Chat dauerhaft ab. |
| **Raid Boost** | 1,99 € | Bevorzugte Platzierung als Raid-Ziel im Netzwerk + Lurker-Steuer-Erinnerung. |
| **Analyse Dashboard** *(empfohlen)* | 1,99 € | Volles Analytics-Dashboard inkl. KI-Auswertung + Lurker-Steuer-Erinnerung. |
| **Werbefrei + Raid Boost** | 3,49 € | Werbefrei und Raid Boost zusammen, günstiger als einzeln. |
| **Werbefrei + Analyse** | 3,49 € | Werbefrei und volles Analytics zusammen. |
| **Analyse + Raid Boost** | 3,49 € | Volles Analytics und Raid Boost zusammen. |
| **Alles drin** | 4,99 € | Alle Features aus allen Plänen gebündelt. |

Die einzelnen Features, die ein bezahlter Plan freischalten kann:

- **Werbefreier Chat** — der Bot postet keine Discord-/Werbe-Nachrichten mehr im eigenen Kanal. Greift auch dann, wenn netzwerkweit gerade eine Aktion/Promo läuft.
- **Bevorzugte Raid-Platzierung (Raid Boost)** — der eigene Kanal wird im Raid-Netzwerk bevorzugt als Ziel vorgeschlagen, also häufiger angeraidet. Wirkt auch dann, wenn man selbst gerade offline ist.
- **Volles Analyse-Dashboard** — Viewer-Verlauf und Peaks pro Stream, Zeitraum-Vergleiche, Wachstumstrends, Follower- und Retention-Übersichten sowie KI-gestützte Auswertung.
- **Lurker-Steuer-Erinnerung** — der Bot erinnert bekannte, gerade anwesende stille Mitleser (Lurker) sanft im Chat. Ein- und ausschaltbar über die Abo-Seite.

## Wann es passiert

- **Feature-Freischaltung:** Sobald ein Plan aktiv wird (nach erfolgreicher Zahlung oder durch einen vom Team gesetzten Plan), sind dessen Features sofort scharf. Bei Plan-Wechsel oder Kündigung passt sich der Funktionsumfang entsprechend an.
- **Bevorzugte Raid-Platzierung:** wirkt laufend, sobald Raid Boost (oder ein Paket damit) aktiv ist — der Kanal wird dann bei der Auswahl von Raid-Zielen im Netzwerk vorne einsortiert.
- **Lurker-Steuer-Erinnerung:** Sie ist nur in Plänen verfügbar, die sie enthalten, und muss zusätzlich vom Streamer eingeschaltet sein. Die Erinnerung wird nur unter bestimmten Bedingungen automatisch in den Chat geschrieben (z. B. wenn der Stream live ist und passende Lurker erkannt wurden). Fehlt dem Bot der nötige Chat-Lesezugriff, feuert das Feature nicht und das Dashboard zeigt einen Hinweis.
- **Testphase:** Neue Nutzer erhalten für eine begrenzte Zeit automatisch erweiterten Zugang, danach fällt das Konto ohne Abo auf den kostenlosen Plan zurück.
- **Provision (Affiliate):** Eine Provision entsteht jedes Mal, wenn ein geworbener und zugeordneter Streamer ein bezahltes Abo bezahlt. Die Auszahlung erfolgt gesammelt, in der Regel über eine monatliche Gutschrift.

## Was Streamer/Viewer sehen

- **Streamer** sehen im Dashboard die Plan-Auswahl mit Preisen (umschaltbar zwischen monatlicher und jährlicher Abrechnung), den eigenen aktuell aktiven Plan, eine Rechnungsübersicht sowie die Abo-Seite mit Profil, Kündigung und den schaltbaren Feature-Optionen ihres Plans.
- Features, die der eigene Plan nicht enthält, erscheinen als **gesperrte Vorschau-Karte** mit dem Hinweis, welches Upgrade sie freischalten würde.
- **Viewer** merken die Pläne nur indirekt: Im werbefreien Plan entfallen die Werbe-Nachrichten des Bots im Chat; bei aktivem Raid Boost erreichen den Kanal tendenziell mehr eingehende Raids; die Lurker-Steuer-Erinnerung erscheint als gelegentliche, sanfte Chat-Nachricht.
- **Affiliates** (Werbepartner) sehen in ihrem eigenen Portal die von ihnen geworbenen Streamer, die aufgelaufenen Provisionen und ihre Gutschriften.

## Was Streamer einstellen können

- **Plan wählen, upgraden, downgraden** über das Dashboard.
- **Abrechnungszeitraum wählen:** monatlich oder jährlich.
- **Profildaten pflegen** (für die Rechnung) und **Rechnungen einsehen**.
- **Abo kündigen** — die Kündigung läuft zum Ende des bereits bezahlten Zeitraums; bis dahin bleiben die Features aktiv, danach fällt der Kanal auf den kostenlosen Plan zurück.
- **Lurker-Steuer-Erinnerung ein-/ausschalten**, sofern der aktive Plan dieses Feature enthält.
- **Am Empfehlungs-Programm teilnehmen:** als Affiliate anmelden, ein Auszahlungs-Konto verbinden und geworbene Streamer dem eigenen Konto zuordnen.

## Grenzen & Sonderfälle

- **Ein Plan pro Streamer.** Es ist immer genau ein Plan aktiv; die enthaltenen Features bestimmen sich aus diesem Plan.
- **Kostenlos bleibt vollwertig moderiert.** Auto-Moderation und automatische Raids sind nie an einen bezahlten Plan gekoppelt — sie laufen im kostenlosen Plan genauso.
- **Vom Team gesetzte Pläne haben Vorrang.** Setzt das Team einem Konto manuell einen Plan (z. B. für eine Sonderregelung oder verlängerte Testphase), gilt dieser vor einem etwaigen Abo. Solche Sonderfälle sind nicht selbst über das Dashboard buchbar.
- **Preise sind Netto-Angaben.** Auf der Plan-Auswahl stehen Netto-Monatspreise.
- **Lurker-Steuer braucht zwei Dinge:** den passenden Plan **und** den aktivierten Schalter **und** den nötigen Chat-Lesezugriff des Bots. Fehlt eines davon, passiert nichts.
- **Empfehlungs-Programm — eine Zuordnung pro Streamer:** Ein geworbener Streamer kann nur einem Affiliate-Konto zugeordnet werden; bereits anderweitig zugeordnete oder schon länger registrierte Streamer lassen sich nicht nachträglich beanspruchen. Ohne verbundenes Auszahlungs-Konto sammeln sich Provisionen nur bis zu einer Obergrenze an, bevor weitere pausiert werden.

## Häufige Fragen

**F: Kostet der Bot etwas?**
A: Nein — die Grundfunktion ist kostenlos. Automatische Raids und die komplette Auto-Moderation laufen ohne Bezahlung. Bezahlte Pläne schalten Zusatz-Features frei (werbefreier Chat, bevorzugte Raid-Platzierung, volles Analyse-Dashboard, Lurker-Steuer-Erinnerung), sind aber optional.

**F: Welche Pläne gibt es und was kosten sie?**
A: Einzelpläne (Werbefrei, Raid Boost, Analyse Dashboard) kosten je 1,99 € netto im Monat. Zweier-Pakete kosten 3,49 €, das Komplettpaket „Alles drin" 4,99 €. Pakete sind günstiger als die enthaltenen Einzelpläne zusammen. Abgerechnet wird wahlweise monatlich oder jährlich.

**F: Was bringt mir Raid Boost konkret?**
A: Dein Kanal wird im Raid-Netzwerk bevorzugt als Raid-Ziel vorgeschlagen, also häufiger angeraidet — auch dann, wenn du selbst gerade nicht online bist. Die genaue Gewichtung der Ziel-Auswahl ist bewusst nicht öffentlich.

**F: Was heißt „Werbefrei"?**
A: Der Bot postet in deinem Chat keine Werbe-/Discord-Nachrichten mehr. Das greift auch, wenn netzwerkweit gerade eine Promo-Aktion läuft. Die automatische Moderation bleibt davon unberührt.

**F: Gibt es eine kostenlose Testphase?**
A: Ja. Neue Nutzer bekommen für eine begrenzte Zeit automatisch erweiterten Zugang, um die bezahlten Features auszuprobieren. Läuft die Testphase ohne Abo aus, fällt das Konto auf den kostenlosen Plan zurück.

**F: Wie kündige ich?**
A: Über die Abo-Seite im Dashboard. Die Kündigung wird zum Ende des bereits bezahlten Zeitraums wirksam — bis dahin bleiben die Features aktiv, danach gilt wieder der kostenlose Plan. Eine Rückerstattung des laufenden Zeitraums ist nicht vorgesehen.

**F: Wo sehe ich meine Rechnungen und ändere meine Rechnungsdaten?**
A: Ebenfalls auf der Abo-Seite im Dashboard. Dort findest du die Rechnungsübersicht und kannst deine Profildaten für die Rechnung pflegen.

**F: Wie funktioniert das Empfehlungs-Programm (Affiliate)?**
A: Du meldest dich als Werbepartner an, verbindest ein Auszahlungs-Konto und ordnest die von dir geworbenen Streamer deinem Konto zu. Für jedes bezahlte Abo dieser Streamer bekommst du eine Provision, die in der Regel monatlich als Gutschrift ausgezahlt wird. Ein Streamer kann nur einem Werbepartner zugeordnet werden, und nur, solange er noch nicht anderweitig zugeordnet oder schon länger registriert ist.

**F: Ich habe ein Feature freigeschaltet, sehe es aber nicht — woran liegt das?**
A: Prüfe zuerst, ob dein aktueller Plan dieses Feature wirklich enthält (sonst zeigt das Dashboard eine gesperrte Vorschau-Karte). Bei der Lurker-Steuer-Erinnerung muss zusätzlich der Schalter aktiviert sein und der Bot den nötigen Chat-Lesezugriff haben — fehlt der Zugriff, weist das Dashboard darauf hin.

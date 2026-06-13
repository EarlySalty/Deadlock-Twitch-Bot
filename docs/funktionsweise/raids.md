# Raids (automatisch & manuell)

## Worum es geht

Ein Raid schickt am Stream-Ende die eigenen Zuschauer gesammelt in den Stream eines anderen Kanals. Der Bot kann das automatisch übernehmen: Wenn ein teilnehmender Deadlock-Streamer offline geht, sucht er einen anderen passenden Deadlock-Kanal, der gerade live ist, und raidet ihn. So bleiben Zuschauer in der Deadlock-Community statt sich zu verlaufen, und die teilnehmenden Streamer schicken sich gegenseitig Publikum zu. Manuelle Raids (vom Streamer selbst gestartet) bleiben jederzeit möglich; der Bot greift hier nur ein, um Raids auf gesperrte Kanäle zu verhindern.

## Was der Bot tut

- **Automatischer Raid am Stream-Ende:** Geht ein teilnehmender Streamer offline, prüft der Bot, ob die gerade beendete Session eine Deadlock-Session war, und startet — wenn ja und alle Voraussetzungen erfüllt sind — automatisch einen Raid auf einen anderen Kanal.
- **Zielauswahl unter Deadlock-Kanälen:** Geraidet werden nur andere teilnehmende Streamer, die gerade live Deadlock spielen. Der Bot bewertet die infrage kommenden Live-Kanäle anhand einer Kombination mehrerer Faktoren und wählt automatisch das beste Ziel. Die genaue Gewichtung ist bewusst nicht dokumentiert.
- **Faire Verteilung:** Der Bot sorgt dafür, dass nicht immer derselbe Kanal die Raids abbekommt. Wer kürzlich erst beraidet wurde, wird eine Weile übersprungen, damit das Publikum breit verteilt wird statt sich auf wenige Kanäle zu konzentrieren.
- **Raid im Namen des Streamers:** Der Raid wird über die einmalig erteilte Berechtigung des Streamers ausgelöst, so als hätte er ihn selbst gestartet. Der Bot raidet nie aus einem fremden Kanal ohne diese Erlaubnis.
- **Erfolg messen:** Nach dem Raid prüft der Bot über mehrere unabhängige Signale, ob der Raid wirklich angekommen ist und ob die Zuschauer im Deadlock-Umfeld geblieben sind. Ein einzelnes Signal reicht nicht — das verhindert, dass Fehlsignale oder abgebrochene Raids fälschlich als Erfolg gezählt werden.
- **Schutz vor Raids auf gesperrte Kanäle:** Der Bot führt eine Sperrliste von Kanälen, die für Raids nicht unterstützt werden. Startet ein Streamer manuell einen Raid auf einen solchen Kanal, versucht der Bot, den Raid sofort wieder abzubrechen, und informiert den Streamer per privater Nachricht.
- **Einladung an externe Deadlock-Streamer:** Raidet ein noch nicht teilnehmender Deadlock-Streamer wiederholt in einen Partner-Kanal herein, schickt der Bot ihm gestufte Einladungs-Nachrichten, um ihn aufs Mitmachen aufmerksam zu machen.

## Wann es passiert

- **Auto-Raid:** Wird ausgelöst, sobald ein teilnehmender Streamer offline geht — aber nur, wenn alle der folgenden Bedingungen erfüllt sind:
  - Der Streamer ist ein aktiver Teilnehmer am Raid-Netzwerk.
  - Die gerade beendete Session war eine Deadlock-Session (es wurde Deadlock gespielt).
  - Der Streamer hat den Bot einmalig autorisiert, in seinem Namen zu raiden.
  - Der automatische Raid ist für diesen Streamer eingeschaltet.
  - Ein passendes Ziel ist verfügbar: ein anderer teilnehmender Kanal, der gerade live Deadlock spielt und nicht erst kürzlich beraidet wurde.
- Fehlt eine dieser Bedingungen, findet kein automatischer Raid statt — der Stream endet dann einfach ohne Raid.
- **Manueller Raid:** Jederzeit vom Streamer selbst über Twitch startbar. Der Bot mischt sich hier nicht in die Zielwahl ein. Erkennt er jedoch, dass das Ziel auf der Raid-Sperrliste steht, greift er ein (Abbruch + Hinweis).
- **Manueller Raid unterdrückt kurzzeitig den Auto-Raid:** Startet ein Streamer kurz vor dem Offline-Gehen selbst einen Raid, setzt der Bot für eine kurze Zeit keinen eigenen automatischen Raid nach, damit nicht zwei Raids kollidieren.

## Was Streamer/Viewer sehen

- **Zuschauer beim Auto-Raid:** Für die Zuschauer sieht ein automatischer Raid genauso aus wie ein normaler, vom Streamer gestarteter Raid — sie werden am Stream-Ende gesammelt in den Zielkanal mitgenommen und landen dort im Live-Stream.
- **Im Zielkanal:** Der raidende Kanal taucht als Raid-Hinweis im Chat des Zielkanals auf, wie bei jedem Twitch-Raid.
- **Raid-Verlauf:** Teilnehmende Streamer können ihre eigene Raid-History einsehen (welche Raids ausgelöst wurden, wohin) sowie Raid-Statistiken/Retention auf dem Dashboard.
- **Sperrlisten-Hinweis:** Raidet ein Streamer manuell auf einen gesperrten Kanal, bekommt er eine private Nachricht (Whisper). Konnte der Raid noch gestoppt werden, lautet sie sinngemäß: „Dein Raid wurde abgebrochen — der Kanal steht bei uns auf der Raid-Blacklist und wird nicht für Raids unterstützt." Lief der Raid schon zu weit, weist die Nachricht darauf hin, dass er nicht mehr stoppbar war und der Kanal künftig gemieden werden sollte.

## Was Streamer einstellen können

- **Berechtigung erteilen:** Der Streamer autorisiert den Bot einmalig über einen Anmelde-Link, damit dieser in seinem Namen raiden darf. Ohne diese Autorisierung findet kein automatischer Raid statt.
- **Auto-Raid ein-/ausschalten:** Der automatische Raid lässt sich pro Streamer aktivieren oder deaktivieren. Ist er deaktiviert, raidet der Bot am Stream-Ende nicht automatisch — manuelle Raids bleiben davon unberührt.
- **Status & Verlauf abrufen:** Der Streamer kann den aktuellen Raid-Status (autorisiert ja/nein, Auto-Raid an/aus) und die eigene Raid-History abfragen.

## Grenzen & Sonderfälle

- **Nur Deadlock:** Auto-Raids laufen ausschließlich von Deadlock-Sessions in andere Deadlock-Kanäle. Wer zuletzt etwas anderes gespielt hat oder bei dem kein passendes Deadlock-Ziel live ist, löst keinen Auto-Raid aus.
- **Kein Ziel verfügbar:** Ist gerade kein passender teilnehmender Kanal live, unterbleibt der Auto-Raid — es wird nicht „irgendwohin" geraidet.
- **Sperrliste lässt sich nicht immer rechtzeitig stoppen:** Der Schutz gegen Raids auf gesperrte Kanäle versucht, einen laufenden Raid abzubrechen. Twitch lässt einen Raid aber nur in einem kurzen Zeitfenster vor dem Start abbrechen — war der Raid schon durchgelaufen, kann der Bot ihn nicht mehr rückgängig machen und weist nur noch per Nachricht darauf hin.
- **Hinweis-Nachricht ist nicht garantiert:** Der Sperrlisten-Hinweis wird als private Twitch-Nachricht verschickt. Twitch stellt solche Nachrichten an fremde Konten nicht immer zu; in dem Fall wirkt der Abbruch trotzdem, nur der Hinweis kommt eventuell nicht an.
- **Erfolg wird konservativ gewertet:** Dass ein Raid technisch ausgelöst wurde, zählt für die Erfolgsmessung noch nicht. Erst wenn mehrere Signale übereinstimmen, gilt ein Raid als bestätigt angekommen.
- **Externe Einladungen sind bewusst geduldig:** Ein externer Streamer, der oft hereinraidet, landet nicht sofort auf der Sperrliste. Es gibt eine Karenzzeit, in der er noch einsteigen oder reagieren kann; wird er in dieser Zeit selbst Teilnehmer, entfällt die Sperrung.

## Häufige Fragen

**Warum hat mein Stream am Ende einen anderen Kanal geraidet, ohne dass ich es gestartet habe?**
Das ist der automatische Raid. Wenn du am Raid-Netzwerk teilnimmst, den Bot autorisiert und den Auto-Raid eingeschaltet hast, sucht der Bot beim Offline-Gehen nach einer Deadlock-Session automatisch einen passenden anderen Deadlock-Kanal und raidet ihn für dich. Möchtest du das nicht, kannst du den Auto-Raid deaktivieren.

**Nach welchen Kriterien sucht der Bot das Raid-Ziel aus?**
Geraidet werden nur andere teilnehmende Kanäle, die gerade live Deadlock spielen. Aus diesen wählt der Bot anhand einer Kombination mehrerer Faktoren automatisch das beste Ziel und achtet dabei auf faire Verteilung, damit nicht immer derselbe Kanal die Raids bekommt. Die genaue Gewichtung legen wir bewusst nicht offen.

**Kann ich den Auto-Raid abschalten?**
Ja. Der automatische Raid lässt sich pro Streamer ein- und ausschalten. Ist er aus, raidet der Bot am Stream-Ende nicht von selbst. Eigene, manuell gestartete Raids sind davon nicht betroffen.

**Kann ich trotzdem selbst raiden, wohin ich will?**
Ja. Manuelle Raids startest du wie gewohnt selbst, der Bot mischt sich bei der Zielwahl nicht ein. Die einzige Ausnahme: Zielst du auf einen Kanal, der bei uns auf der Raid-Sperrliste steht, versucht der Bot den Raid abzubrechen und schickt dir dazu eine Erklärung.

**Ich habe eine Nachricht bekommen, dass mein Raid abgebrochen wurde — was bedeutet das?**
Das Ziel deines Raids steht auf unserer Raid-Sperrliste (Kanäle, die wir nicht für Raids unterstützen). Der Bot hat den Raid deshalb abgebrochen. Konnte er nicht mehr rechtzeitig gestoppt werden, ist er durchgelaufen — dann bittet dich die Nachricht, diesen Kanal künftig nicht mehr als Raid-Ziel zu nehmen.

**Warum hat der Bot keinen Raid gemacht, obwohl ich offline gegangen bin?**
Ein automatischer Raid braucht mehrere Voraussetzungen: aktive Teilnahme am Netzwerk, eine gerade beendete Deadlock-Session, eine erteilte Berechtigung, den eingeschalteten Auto-Raid und ein passendes Live-Ziel. Fehlt davon etwas — etwa wenn gerade kein anderer teilnehmender Deadlock-Kanal live ist — endet der Stream ohne Raid.

**Müssen die Zuschauer beim automatischen Raid etwas tun?**
Nein. Für die Zuschauer fühlt sich ein automatischer Raid genau wie ein normaler Twitch-Raid an: Sie werden am Stream-Ende gesammelt in den Zielkanal mitgenommen.

**Ich bin selbst kein Teilnehmer, der Bot hat mir aber eine Einladung geschickt — warum?**
Wenn du als Deadlock-Streamer wiederholt in einen teilnehmenden Kanal hereingeraidet bist, macht dich der Bot über eine kurze Nachricht auf die Möglichkeit aufmerksam, selbst mitzumachen. Das ist eine Einladung, keine Verpflichtung.

# Chat-Bot, Moderation & Befehle

## Worum es geht

Der Bot ist als eigener Twitch-Account in den Kanälen der Partner-Streamer aktiv. Er moderiert den Chat automatisch, schützt vor Werbe- und Viewer-Bots, postet von Zeit zu Zeit Hinweise auf die Community, warnt vor verdächtigen Fremd-Angeboten im Chat und reagiert auf eine Reihe von Chat-Befehlen. Dieses Kapitel beschreibt, was Zuschauer, Mods und Streamer im Chat tatsächlich sehen und bedienen können.

## Was der Bot tut

- **Moderiert automatisch und durchgehend.** Sobald der Bot in einem Partner-Kanal ist, läuft die Moderation immer mit. Sie lässt sich nicht "pausieren" oder feiner einstellen — sie ist bewusst ein fester Schutz, keine optionale Funktion.
- **Schützt vor Werbe- und Viewer-Bots.** Der Bot erkennt typische Spam- und Bot-Nachrichten (etwa Fremdwerbung, Account-Kauf-Angebote, kopierte Massen-Nachrichten) und entfernt sie. Bei klaren Treffern wird die Nachricht gelöscht und der Absender gebannt.
- **Erkennt Verschleierungs-Tricks.** Spammer versuchen oft, ihre Nachrichten so zu verändern, dass einfache Filter sie nicht erkennen. Der Bot durchschaut solche Verschleierungs-Versuche, damit der Trick nicht funktioniert.
- **Warnt vor verdächtigen Fremd-Angeboten ("Fake-Server-Warnung").** Wenn ein Chatter im Stream eine fremde Dienstleistung oder einen fremden Server anzupreisen versucht, kann der Bot im Chat einen kurzen, vorsichtig formulierten Hinweis posten, dass das ein Betrugsversuch sein könnte. Der Hinweis nennt keine harten Vorwürfe, sondern macht Zuschauer aufmerksam.
- **Postet Community-Hinweise (Promos).** Während eines laufenden Streams platziert der Bot ab und zu eine kurze Nachricht, die auf den Community-Discord des Streamers verweist. Diese Hinweise kommen nicht ständig, sondern in größeren Abständen und nur, wenn im Chat überhaupt Betrieb herrscht.
- **Trackt anwesende Zuschauer (Lurker/Presence).** Der Bot merkt sich, wer gerade im Kanal anwesend ist, auch wenn diese Person nicht schreibt. Diese Anwesenheits-Daten nutzt unter anderem die "Lurker Steuer" (siehe weiter unten).
- **Reagiert auf Chat-Befehle.** Streamer, Mods und Zuschauer können den Bot über Befehle steuern oder abfragen (vollständige Liste unten).
- **Reagiert in laufenden Deadlock-Streams gelegentlich auf "Danke".** Schreibt jemand dem Bot ein kurzes Dankeschön, kann er mit einer frechen Kurzantwort reagieren. Das ist eine optionale Spielerei, kommt nur in größeren Abständen und ist standardmäßig zurückhaltend.

## Wann es passiert

- **Moderation:** läuft bei jeder eingehenden Chat-Nachricht in einem betreuten Kanal — unabhängig davon, ob der Stream live ist.
- **Bann/Löschung:** nur dann, wenn der Bot eine Nachricht als eindeutigen Spam-/Bot-Treffer einstuft. Die Erkennung verdichtet mehrere Hinweise zu einer Gesamteinschätzung und ist bewusst konservativ ausgelegt, sodass echte Zuschauer praktisch nie fälschlich getroffen werden.
- **Fake-Server-Warnung:** wird ausgelöst, wenn das Verhalten eines Chatters (z. B. ein typischer Anwerbe-Pitch) auf einen Betrugsversuch hindeutet. Die genauen Auslöse-Kriterien sind bewusst nicht öffentlich.
- **Promos:** werden während eines laufenden Streams in größeren Abständen gesendet und nur, wenn seit der letzten Promo genug frische Chat-Aktivität da war. Steht gerade eine Fake-Server-Warnung an, ersetzt sie in diesem Moment die Promo (es kommt nicht beides direkt hintereinander).
- **Befehle:** wirken sofort, sobald jemand sie in den Chat schreibt. Bei Befehlen mit Rechte-Beschränkung prüft der Bot vorher, ob die Person dazu berechtigt ist.

## Was Streamer/Viewer sehen

- **Bei einem Bann:** Die Spam-Nachricht verschwindet und der Absender ist gebannt. Standardmäßig kann der Bot dazu eine kurze Notiz im Chat hinterlassen — diese Benachrichtigung lässt sich pro Kanal abschalten (siehe `!silentban`). Der Bann selbst bleibt immer aktiv, egal ob die Notiz an oder aus ist.
- **Bei einer Fake-Server-Warnung:** Eine kurze Hinweis-Nachricht im Chat, die Zuschauer vorsichtig vor einem möglichen Betrugsversuch warnt. Der Text wechselt, damit nie zweimal hintereinander derselbe Wortlaut erscheint.
- **Bei einer Promo:** Eine kurze, locker formulierte Nachricht mit Verweis auf den Community-Discord.
- **Keine Links im Chat:** Twitch blockiert über AutoMod oft fremde Links. Deshalb postet der Bot in seinen automatischen Nachrichten keine nackten URLs, sondern verweist über die Profil-Bio bzw. den Einlade-Mechanismus. (Befehle wie `!dldc` oder `!invite`, bei denen ein Zuschauer aktiv nach dem Link fragt, geben den Discord-Link direkt aus.)
- **Bei Befehlen:** eine direkte Antwort des Bots im Chat, meist mit `@Name` an die Person, die den Befehl ausgelöst hat.

## Befehlsübersicht

Hinweis zu Rechten: "Broadcaster" = der Streamer selbst, "Mods" = die Moderatoren des Kanals, "Alle" = jeder Zuschauer. Befehle funktionieren nur in Kanälen, die als Partner registriert sind; in nicht registrierten Kanälen antwortet der Bot mit einem entsprechenden Hinweis oder bleibt still.

### Befehle für alle Zuschauer

- **`!ping`** (auch `!health`, `!status`, `!bot`) — Prüft, ob der Bot online ist. Antwortet mit einer zufälligen, lockeren "Ich lebe noch"-Meldung.
- **`!clip`** (auch `!createclip`) — Erstellt einen Clip aus dem aktuellen Stream (ungefähr die letzten 60 Sekunden). Optional kann ein Titel mitgegeben werden, z. B. `!clip Unfassbarer Outplay`; ohne Titel wählt der Bot einen Standard-Titel. Der Bot antwortet mit dem Link zum erstellten Clip.
- **`!dldc`** (auch `!dlde`) — Gibt den hinterlegten Discord-Invite-Link des Streamers aus. Ist kein Link hinterlegt, sagt der Bot das.
- **`!invite`** — Gibt im laufenden Deadlock-Stream den Community-Discord-Einladungslink aus. Pro Person und Kanal nur einmal innerhalb eines bestimmten Zeitfensters, danach wieder.
- **`!raid_status`** (auch `!raidbot_status`) — Zeigt an, ob der Auto-Raid für diesen Kanal aktiv ist, plus eine kurze Raid-Statistik und den letzten Raid.
- **`!raid_history`** (auch `!raidbot_history`) — Zeigt die letzten Raids des Kanals.
- **`!engagement_status`** — Zeigt, ob der AI-Engagement-Layer für den Kanal an oder aus ist, und wann er zuletzt etwas getan hat.
- **`!engagement_ignore_me`** — Persönliches Opt-out: Die AI ignoriert ab sofort die Nachrichten der Person, die den Befehl schreibt.
- **`!engagement_remember_me`** — Nimmt das persönliche Opt-out wieder zurück.

### Befehle für Broadcaster und Mods

- **`!raid_enable`** (auch `!raidbot`) — Aktiviert den Auto-Raid. Ist der Kanal noch nicht autorisiert, weist der Bot darauf hin, dass der Streamer den Bot erst über den Autorisierungs-/Anmelde-Link autorisieren muss.
- **`!raid`** (auch `!traid`) — Startet sofort einen Raid auf den bestmöglichen passenden Partner — genau wie der Auto-Raid es täte. Funktioniert nur, wenn der Kanal gerade Deadlock streamt (oder eben erst von Deadlock auf "Just Chatting" gewechselt ist) und ein passendes Ziel live ist. Der Bot meldet zurück, auf wen geraidet wird, oder warum gerade kein Raid möglich ist.
- **`!uban`** (auch `!unban`) — Hebt den letzten automatischen Bann in diesem Kanal wieder auf. Praktisch, falls die Moderation ausnahmsweise daneben lag.
- **`!silentban`** — Schaltet die Chat-Benachrichtigung bei Auto-Bans für diesen Kanal an oder aus. Wichtig: Die Bans werden weiterhin ausgeführt — nur die Notiz im Chat entfällt.
- **`!silentraid`** — Schaltet die Chat-Benachrichtigung bei Raids für diesen Kanal an oder aus. Die Raids laufen weiter, nur die Ansage entfällt.
- **`!title`** (auch `!titel`) — Generiert einen Vorschlag für den Stream-Titel. Beispiel: `!title ranked solo grind`. Der Bot liefert einen Hauptvorschlag und ggf. Alternativen. Mit dem Zusatz `--live` kann der aktuelle Live-Status einbezogen werden.
- **`!engagement_on`** / **`!engagement_off`** — Schaltet den AI-Engagement-Layer für den Kanal ein bzw. aus. (Auch eine vom Streamer ernannte "Super-Mod"-Rolle darf das schalten.) Eingeschaltet schaltet sich der Layer bei Stream-Ende automatisch wieder ab.

### Nur für den Broadcaster

- **`!lurkersteuer_off`** (auch `!lurkersteuer_aus`, `!lurker_tax_off`) — Deaktiviert die "Lurker Steuer" für den Kanal dauerhaft. Die Wieder-Aktivierung läuft über den Abo-Bereich im Dashboard, nicht über den Chat. Die Lurker Steuer ist nur in bezahlten Plänen verfügbar.

## Lurker Steuer (Erinnerung an stille Dauer-Zuschauer)

Die "Lurker Steuer" ist eine optionale Funktion für bezahlte Pläne. Sie erinnert bekannte, gerade anwesende Dauer-Zuschauer ("Lurker") sanft im Chat — also Leute, die regelmäßig still mitschauen, ohne zu schreiben. Der Ton bleibt bewusst weich, ohne Druck oder Bloßstellen; es werden keine Punktestände oder Belohnungs-Kosten behauptet.

- **Wann:** nur im laufenden Stream, nur bei bezahltem Plan und nur, wenn die Funktion eingeschaltet ist und der Bot die nötigen Anwesenheits-Daten hat.
- **Wen:** Personen, die aktuell still anwesend sind und über frühere Streams hinweg eine erkennbare Lurk-Historie auf diesem Kanal haben.
- **Wie oft:** Pro Live-Session wird dieselbe Person nur einmal direkt erwähnt, und es werden höchstens zwei Namen pro Erinnerung genannt. Die Erinnerung teilt sich denselben Sende-Rhythmus wie die normalen Promo-/Discord-Nachrichten — sie kommt also nicht zusätzlich obendrauf.

## Was Streamer einstellen können

- **Auto-Ban-Benachrichtigung** im Chat an/aus: per `!silentban`.
- **Raid-Benachrichtigung** im Chat an/aus: per `!silentraid`.
- **Auto-Raid** an/aus: per `!raid_enable` bzw. über das Dashboard (Voraussetzung: Bot autorisiert).
- **AI-Engagement-Layer** pro Kanal an/aus: per `!engagement_on` / `!engagement_off`.
- **Lurker Steuer** abschalten per `!lurkersteuer_off`, wieder einschalten im Abo-Bereich des Dashboards.
- **Eigene Promo-Nachricht:** Streamer können einen eigenen Promo-Text hinterlegen (über das Dashboard). Dieser darf maximal 500 Zeichen lang sein und muss den Platzhalter `{invite}` enthalten, an dessen Stelle dann der Einladungslink eingesetzt wird.
- **Was nicht einstellbar ist:** Die automatische Moderation selbst (Spam-/Bot-Schutz) lässt sich nicht abschalten oder in der Schärfe verändern. Sie ist immer an. Abschaltbar ist lediglich die Chat-*Notiz* zu Bans, nicht der Schutz.

## Grenzen & Sonderfälle

- **Nur Partner-Kanäle:** Befehle und Moderation greifen nur in Kanälen, die als Partner registriert sind. In anderen Kanälen verweist der Bot bei Befehlen darauf bzw. reagiert nicht.
- **Bot braucht Rechte:** Damit der Bot moderieren und Nachrichten senden kann, muss er im Kanal die nötigen Rechte haben. Verliert er sie (z. B. wird selbst gebannt oder ist kein Mod), stellt er das Senden in diesem Kanal automatisch ein, statt vergeblich weiterzuversuchen.
- **Re-Autorisierung:** Für manche Aktionen (Raid, Silent-Toggles) muss die Autorisierung des Streamers aktuell sein. Ist eine Neu-Autorisierung fällig, weist der Bot darauf hin (per Discord-DM oder über den Autorisierungs-Link).
- **`!raid` ist an Deadlock gebunden:** Ein manueller Raid funktioniert nur, wenn gerade Deadlock gestreamt wird (oder eben erst von Deadlock auf "Just Chatting" gewechselt wurde) und ein passendes deutsches Deadlock-Ziel live ist. Sonst meldet der Bot, dass kein Raid möglich ist.
- **Fehlbann ist sehr selten, aber Korrektur ist da:** Die Moderation ist konservativ ausgelegt, damit echte Zuschauer praktisch nie getroffen werden. Sollte es doch passieren, hebt `!uban` den letzten Auto-Bann auf.
- **Verbessert sich über die Zeit:** Der Spam-Schutz wird laufend nachgeschärft, damit er neue Spam-Versuche zuverlässiger erkennt. Ein versehentlicher Treffer lässt sich jederzeit mit `!uban` sofort zurücknehmen.
- **AI-Engagement endet mit dem Stream:** Eingeschaltetes AI-Engagement schaltet sich beim Stream-Ende automatisch wieder ab und muss bei Bedarf erneut aktiviert werden.

## Häufige Fragen

**F: Warum wurde jemand in meinem Chat automatisch gebannt?**
A: Der Bot hat die Nachricht als eindeutigen Spam- oder Bot-Versuch eingestuft (z. B. Fremdwerbung, Account-Verkauf oder kopierte Massen-Nachrichten) und sie deshalb gelöscht und den Absender gebannt. Die Erkennung ist bewusst vorsichtig, damit echte Zuschauer praktisch nie betroffen sind. Wenn es ausnahmsweise doch ein Fehlgriff war, kann der Streamer oder ein Mod mit `!uban` den letzten Auto-Bann zurücknehmen.

**F: Kann ich die automatische Moderation abschalten oder lockerer stellen?**
A: Nein. Der Spam- und Bot-Schutz ist bewusst immer an und nicht in der Schärfe einstellbar — das ist der Kern des Schutzes. Abschalten lässt sich nur die *Benachrichtigung* im Chat bei Bans (per `!silentban`); die Bans selbst laufen weiter.

**F: Warum postet der Bot keinen Link, sondern verweist auf die Bio?**
A: Twitch blockiert über AutoMod häufig fremde Links, sodass solche Nachrichten verschwinden würden. Deshalb verweist der Bot in seinen automatischen Hinweisen auf die Profil-Bio statt einen nackten Link zu posten. Wenn ein Zuschauer aktiv nach dem Discord fragt (`!dldc` oder `!invite`), gibt der Bot den Link direkt aus.

**F: Warum kommt manchmal eine Warnung vor einem "Server" oder Angebot im Chat?**
A: Das ist die Fake-Server-Warnung. Wenn jemand im Chat eine fremde Dienstleistung oder einen fremden Server anpreist und das Verhalten nach einem Betrugsversuch aussieht, macht der Bot Zuschauer vorsichtig darauf aufmerksam. Die Formulierung bleibt bewusst zurückhaltend ("könnte ein Betrugsversuch sein") und kein harter Vorwurf.

**F: Wie erstelle ich einen Clip über den Chat?**
A: Schreib `!clip` in den Chat — der Bot erstellt einen Clip aus den letzten rund 60 Sekunden und schickt dir den Link. Du kannst einen Titel mitgeben, z. B. `!clip Bester Outplay heute`.

**F: Wie sehe ich, ob der Auto-Raid an ist?**
A: Mit `!raid_status` zeigt der Bot, ob der Auto-Raid für den Kanal aktiv ist, plus eine kurze Statistik und den letzten Raid. Aktivieren geht mit `!raid_enable` (nur Broadcaster/Mods, Bot muss autorisiert sein).

**F: Wie schalte ich die Lurker Steuer wieder ein, nachdem ich sie per Chat abgeschaltet habe?**
A: Die Reaktivierung läuft nicht über den Chat, sondern über den Abo-Bereich im Dashboard. Per `!lurkersteuer_off` lässt sie sich nur abschalten. Die Funktion ist außerdem nur in bezahlten Plänen verfügbar.

**F: Was bedeutet der AI-Engagement-Layer und wie steuere ich ihn?**
A: Das ist die optionale KI-Konversations-Funktion des Bots. Sie lässt sich pro Kanal mit `!engagement_on` / `!engagement_off` schalten (Broadcaster, Mods oder Super-Mod). Einzelne Zuschauer können sich mit `!engagement_ignore_me` selbst ausnehmen und mit `!engagement_remember_me` wieder einschließen. Eingeschaltet schaltet sich der Layer bei Stream-Ende automatisch wieder ab.

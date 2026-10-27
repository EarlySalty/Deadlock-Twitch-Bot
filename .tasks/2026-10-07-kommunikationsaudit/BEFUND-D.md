# Paket D: Bot-Nachrichten, Medien und Uplink-Einstiege

Rechercheagent: a064032b72ce7f599. Hauptsession hat die Agentenübergabe übernommen. Grundlage: Snapshot `9315b3cf7e4a6feeffc32b603db26eca78b03a2c`, Pfade relativ zum Repository, gelesen unter `/tmp/twitch-kommunikationsaudit-20261007/`. Graphify vor Codeprüfung. Keine Live-Kontoaktionen, Nachrichten oder Tests. Die beschriebenen Missverständnisse sind fachliche Einschätzungen, keine Ergebnisse einer Nutzerstudie.

## D-01: Clip-Titel wird bestätigt, aber nicht gesetzt

P2, `!clip <Titel>`. „Clip erstellt“ nennt den eingegebenen Titel. Tatsächlich dient dieser Text der Chatantwort; der verdrahtete Clip-Aufruf erhält Kanal-ID und Kanalname, keinen Titel.

Belege: `rust/crates/tb-chat/src/commands.rs:1847,1874`; `rust/bin/tb-bot/src/chat_wiring.rs:130,156`.

Korrektur: Titel tatsächlich setzen oder ausdrücklich als Kommentar zum Clip kennzeichnen. Text/Verhalten, statisch verbunden.

## D-02: Clip-Fehler liefern falsche Zuständigkeit und pauschale Wiederholung

P2. „OAuth fehlt. Bitte den Bot einmal autorisieren“ und „Bitte in 10 Sekunden nochmal versuchen.“ Zuschauer können den Kanalzugang nicht selbst reparieren. Ein fehlender Kanalzugang kann zunächst durch Bot-Zugang ersetzt werden; erst dessen Ausfall ergibt `OAuthMissing`. Unterschiedliche Twitch-Fehler landen gemeinsam in `Failed`. Fehlende Rechte verschwinden nicht durch zehn Sekunden Warten.

Belege: `rust/crates/tb-chat/src/commands.rs:75`; `rust/bin/tb-bot/src/chat_wiring.rs:134-154,236`.

Korrektur: Zuständigkeit und konkreten Dashboard-Weg für Streamer nennen. Vorübergehende Fehler von fehlender Einrichtung unterscheiden. Text/Fehlerbehandlung.

## D-03: „AI ignoriert dich“ übertreibt die Wirkung

P1, `!engagement_ignore_me`. „OK, AI ignoriert dich ab sofort.“ Gespeichert wird eine Ausnahme für Engagement. Brain-Chat besitzt einen anderen Antwortpfad ohne diese Prüfung. Laut Wissenshilfe laufen gewöhnliche Chat- und Statistikaufzeichnungen weiter.

Belege: `rust/crates/tb-chat/src/commands.rs:2447`; `rust/crates/tb-engagement/src/gate.rs:71`; `rust/bin/tb-bot/src/brain_chat_wiring.rs:368`; `rust/knowledge/bot/chat-befehle.md:63`.

Korrektur: Bestätigung auf tatsächlich abgeschaltete Engagement-Antworten begrenzen und verbleibende Aufzeichnungen nennen. Ausdehnung auf weitere Antworten gemeinsam entscheiden. Text/Datennutzung/Produktentscheidung.

## D-04: Hilfedialog erklärt Aufzeichnung und mögliche Weitergabe nicht am Eingabefeld

P1, öffentliche Bot-Hilfe und Concierge. „Deine Frage …“, „Fragen zur Einrichtung und den Funktionen“. Der Handler protokolliert Frage, Antwort und Netzwerkgegenstelle. Wenn der interne Versandzugang vorhanden ist, wird zusätzlich ein Discord-Protokoll angestoßen, dessen Embed auch die Gegenstelle enthält. Direkt am Eingabefeld fehlt die Erläuterung.

Belege: `website/src/components/layout/SiteChatbot.tsx:226`; `rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:891,710,639`.

Korrektur: Vor dem Absenden knapp sagen, was gespeichert wird und wer zur Betreuung mitlesen kann. Aufbewahrung und Erforderlichkeit der Netzwerkadresse gesondert prüfen. Datennutzung/Text. Implementierter Pfad belegt; tatsächlicher Discord-Versand und rechtliche Vollständigkeit bestehender Datenschutzhinweise nicht geprüft.

## D-05: Selbst-Erklärung verspricht unbelegbare Fehlerfreiheit

P2, Bot-Frage und Concierge. „die Antwort kommt nur aus dem, was der Bot wirklich macht“ sowie „Was nicht in der Hausakte steht, erfindet er nicht“. Antworten entstehen aus Wissensmaterial über einen konfigurationsabhängigen Legacy- oder Brain-Pfad. Im Legacy-Pfad erhält eine brauchbar erscheinende Modellausgabe die Einstufung `grounded`, ohne jede Aussage gegen Rust-Code zu prüfen.

Belege: `website/src/components/sections/BotQuestionBox.tsx:93`; `website/src/pages/FaqDoormanPage.tsx:192`; `rust/crates/tb-dashboard-api/src/handlers/self_explainer.rs:325,857`.

Korrektur: Hilfe aus Dokumentation mit möglichen Fehlern beschreiben, Quellen anbieten, keine Erfindungsfreiheit garantieren. Text. Kein falscher Live-Dialog nachgewiesen.

## D-06: Direkte Fragen können still verschwinden

P2, direkt an den Bot gerichtete Chatfrage. Bei fehlendem Antwortdienst, ausgeschöpften Grenzen oder Protokollfehlern bleibt die Antwort aus. Der Brain-Pfad kann die Nachricht trotzdem übernehmen, sodass kein nachfolgender Antwortpfad antwortet.

Belege: `rust/bin/tb-bot/src/brain_chat_wiring.rs:390-427,447`; `rust/crates/tb-chat/src/pipeline.rs:1104`.

Korrektur: Für ausdrücklich gestellte Fragen eine knappe, entprellte Ausfallmeldung oder einen Hilfeweg anbieten. Absichtliches Stummschalten und Wiederholungen nicht zuspammen. Verhalten/Bedienung, gilt bei aktiviertem Brain-Chat; produktive Aktivierung nicht geprüft.

## D-07: „Automatisch posten“ aus stoppt bestehende Planungen nicht

P1, `/social-media-admin`, Zeitplan. Der Schalter „Automatisch posten“ ändert die Planungseinstellung. Bereits vorhandene Warteschlangeneinträge bleiben bestehen. Der Uploader prüft Freigabe und Fälligkeit, nicht erneut diesen Schalter. „Post stoppen“ ist eine eigene Aktion.

Belege: `bot/dashboard_v2/src/pages/SocialMedia.tsx:1593`; `rust/crates/tb-social-media/src/posting_plan.rs:322`; `rust/crates/tb-social-media/src/clip_queue.rs:278`; `rust/crates/tb-social-media/src/upload_worker.rs:142`.

Korrektur: Neue Planung und bereits geplante Veröffentlichungen beim Ausschalten unterscheiden. Betroffene Posts zählen und Stopp-Weg anbieten. Ein umfassender Pausenknopf wäre eine Verhaltensentscheidung. Text/Bedienung/Produktentscheidung.

## D-08: Zwei Automatikmodi ohne klaren Unterschied im Ablauf

P2, Social-Media-Freigabe. „Einspruch bis zum Termin“ gegenüber „Vollautomatik“. Beide erlauben automatische Freigabe ohne Sichtung. Die Stoppfunktion prüft keinen dieser Modi und wird anhand des Clipzustands eingeblendet. Im geprüften Ablauf ist kein eigenes Mindestfenster für Einspruch erkennbar.

Belege: `bot/dashboard_v2/src/components/socialmedia/labels.ts:76`; `rust/crates/tb-social-media/src/posting_plan.rs:58`; `rust/crates/tb-social-media/src/approval.rs:755`; `bot/dashboard_v2/src/pages/SocialMedia.tsx:2543`.

Korrektur: Beabsichtigten Unterschied samt Vorlauf und Benachrichtigung definieren oder Modi zusammenführen. Produktentscheidung/Bedienung. Keine Behauptung, dass bereits eine konkrete zugesicherte Frist verletzt wurde.

## D-09: Plattform-Trennung kann Uploads über ein gemeinsames Konto zulassen

P1, Social-Media-Verbindungen. „Trennen“, „Der gespeicherte Kontozugang wurde gelöscht …“, „Uploads brauchen danach eine verfügbare Verbindung.“ Entfernt wird der eigene Zugang. Der Uploader kann auf eine verfügbare globale Sammelverbindung zurückfallen. Warteschlange und Freigaben werden durch Trennung nicht gelöscht.

Belege: `bot/dashboard_v2/src/pages/SocialMedia.tsx:1961,2035`; `rust/crates/tb-social-media/src/disconnect.rs:22`; `rust/crates/tb-social-media/src/credentials.rs:88,117`; `rust/crates/tb-social-media/src/upload_worker.rs:820`.

Korrektur: Vor Trennung konkret zeigen, ob ein gemeinsames Konto übernehmen kann. Wirklichen Veröffentlichungsstopp getrennt anbieten. Automatischen Kontowechsel bewusst entscheiden. Text/Bedienung/Produktentscheidung; Rückfall hängt von vorhandenen gültigen Zugängen ab.

## D-10: VOD-Archiv verschweigt Umfang und Grenzen

P1, Social-Media-VOD-Archiv. „Automatisch sichern“, „Sichtbarkeit auf YouTube“. Der Worker entdeckt auch vorhandene Twitch-Aufzeichnungen. Ohne eigenen YouTube-Zugang kann er lokal herunterladen und auf eine spätere Verbindung warten. Einstellungen werden zu Beginn eines Durchlaufs geladen, Platzmangel kann den Lauf aussetzen. Die Karte zeigt weder letzte erfolgreiche Sicherung noch wartende oder fehlgeschlagene VODs.

Belege: `bot/dashboard_v2/src/pages/SocialMedia.tsx:2096`; `rust/crates/tb-vod-archive/src/worker.rs:275,302,322,457,677`.

Korrektur: Vor Aktivierung erklären, dass vorhandene Aufzeichnungen berücksichtigt und lokal zwischengespeichert werden können. YouTube-Voraussetzung, tatsächlichen Sicherungsstatus und Wirkung des Ausschaltens zeigen. Text/Bedienung. Keine aktuell gesicherten Produktions-VODs geprüft.

## D-11: VOD-Lesefehler sieht wie ausgeschaltetes Archiv aus

P1, VOD-Karte. Nicht gesetzter Schalter, Sichtbarkeit „Privat“, kein Ladefehler. Datenbankfehler werden durch `.ok().flatten()` verworfen und in Standardwerte übersetzt. Der Handler liefert sie als reguläres Ergebnis, weshalb die vorhandene Frontend-Fehleranzeige nicht greifen kann.

Belege: `rust/crates/tb-social-media/src/vod_archive.rs:50-89`; `rust/crates/tb-dashboard-api/src/handlers/social_media.rs:3612`; `bot/dashboard_v2/src/pages/SocialMedia.tsx:2086`.

Korrektur: Unbekannten Zustand als Fehler durchreichen und nicht als „Aus“ darstellen. Änderungen bis erfolgreichem Abruf sperren. Statusverhalten, kein bloßer Textfehler.

## D-12: YouTube-Verbindungen nicht ausreichend nach Zweck benannt

P2, Wechsel zwischen Uplink und Social Media. Uplink meldet „Verbunden“, Social Media verlangt erneut „Verbinden“. Uplink speichert seine eigene Plattformverbindung; Clip-Uploads und VOD-Archiv nutzen den Social-Media-Zugang. Das VOD-Archiv verlangt einen eigenen Kanalzugang.

Belege: `bot/dashboard_v2/src/api/uplink.ts:235`; `rust/crates/tb-dashboard-api/src/handlers/plattform_connect.rs:443`; `rust/crates/tb-social-media/src/credentials.rs:117`; `rust/crates/tb-vod-archive/src/worker.rs:134`.

Korrektur: Verbindung nach Zweck benennen, etwa Livestream gegenüber Clip-/VOD-Upload, und zur fehlenden Freigabe verlinken. Kein zusätzlicher technischer Verbindungsweg vorgeschlagen. Text/Bedienung.

## D-13: Freigabe betrifft Veröffentlichung, nicht vorbereitenden Download

P2, Freigabetext „Jeder Clip wartet auf dein Okay.“ Geeignete vorhandene Clips können unabhängig von der Veröffentlichung bereits lokal heruntergeladen werden. Die Transkriptions-/Anreicherungspipeline ist im geprüften Bot-Einstieg dagegen abgeschaltet.

Belege: `bot/dashboard_v2/src/components/socialmedia/labels.ts:77`; `rust/bin/tb-bot/src/main.rs:1709-1732`; `rust/crates/tb-social-media/src/clip_prep_worker.rs:147`.

Korrektur: „Veröffentlichung nur nach Freigabe“ präzisieren und lokale Vorbereitung mit Löschweg erklären. Keine aktive Transkription behaupten. Text/Datennutzung. Dies ist ein Präzisierungsvorschlag, kein Beleg für unerlaubte Veröffentlichung.

## Abdeckung und Grenzen

Geprüft: ausgewählte Chatbefehle samt Verdrahtung, Einladungen, Engagement-Bestätigungen, Brain-Chat, öffentliche Selbst-Erklärung, Social-Media-Freigaben und Uploads, VOD-Archiv, Uplink-Verbindungen und OBS-Einstiege.

Positiv: Abo-Erinnerung kündigt öffentliche Chatnachricht an. Uplink warnt vor dem Teilen von OBS-Adressen und erklärt deren Neuerzeugung. Social Media unterscheidet wartende Verbindungen, TikTok-Postfach und tatsächliche YouTube-Sichtbarkeit.

Nicht geprüft: sämtliche Streamer-Benachrichtigungen, reale Konten/Rechte/Nachrichten, Produktionskonfiguration, Browserdarstellung, kompletter Relay-Ablauf und externe Plattformreaktionen. Raid-/Werbefunktionen wurden nicht erneut in der Tiefe geprüft. Kein vollständiger Live-Kommunikationsnachweis.

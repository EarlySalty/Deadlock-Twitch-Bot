# Social-Media-Dashboard + Deploy-Nacht 2026-08-15 — offene Punkte

Stand beim Schreiben: Bot laeuft stabil auf dem 99bc323e-Deploy, Dashboard
Stufe 1 ist live (Seite "Social Media", Zeitplan/Modi/Kategorien/Vorratswarnung).
Branch feature/vod-auto-save ist auf 08cc9031 gepusht.

## DEPLOY GESTOPPT (Entscheidung 06:0x, wartet auf wachen Menschen)

Der Sammel-Deploy von 08cc9031 ist AUSGESETZT. Grund (Beleg Live-Log 06:01:11,
global_ban_sweep miracleghost9): Der Bot liest dort aktiv den Chat mit und
scheitert nur an fehlenden Mod-Rechten. Twitchs `400 "user is banned"` auf der
Moderator-Einsetzung beweist also KEINEN Bann; der Nutzer hatte von Anfang an
recht. c246502a (Reaktion wieder scharf) und 96d48afd (haengt am selben Enum)
duerfen so nicht live, sonst pausiert der Bot wieder Kanaele auf Basis eines
Signals ohne Beweiskraft.

- Live und unkritisch bleibt 99bc323e: aktive Pruefung meldet nur, pausiert nicht.
- [ ] Ban-Klassifikation neu herleiten: was bedeutet der 400-Body wirklich
      (Session c8 analysiert, Auflage: kein Deploy, keine DMs, keine
      Prod-DB-Schreibzugriffe).
- [ ] bot_banned-Marker fuer whysolowkey und pixelpiratemarvin ueberpruefen
      (stammen aus dem eventsub-Pfad, nicht aus der Probe).
- [ ] Erst nach geklaerter Klassifikation: Sammel-Deploy neu schnueren
      (Versions-Guard und Test-Fixes sind unstrittig und koennen notfalls
      separat deployt werden). ACHTUNG: der Fireworks-Default 0731 ist NICHT
      mehr unstrittig, siehe naechster Punkt.
- [ ] Fireworks-Default 0731 aus dem Sammel-Deploy nehmen. Gemessen am
      2026-08-15: accounts/fireworks/models/deepseek-v4-flash-0731 wird bei
      Fireworks nicht warm gehalten, erster Aufruf 60 s Timeout, zweiter
      14,8 s, erst danach 0,6 s. Der Deadlock-Concierge (8-s-Limit) ist daran
      einen halben Tag lang komplett ausgefallen. Als Default taugt nur ein
      dauerhaft warmes Modell. Ebenfalls nicht nehmen: kimi-k2p6, ein
      Thinking-Modell, das sein Token-Budget im Reasoning verbraucht und bei
      knappem max_tokens einen leeren content liefert (hat Hermes lahmgelegt).
      Brauchbar gemessen: kimi-k3, 0,9 s blank, 4,3 bis 9,2 s mit 4 kB
      Systemprompt und JSON-Modus.

## Wartet auf andere

- [ ] d2s LLM-Modell-Resolver (feature/llm-modell-resolver): Review-Gate BLOCK
      mit 3 Befunden (pick_newest: None<Some-Ordering loest aufs tote Modell auf;
      invalidate vor Refresh leert die Zelle bei transienten Fehlern; 404-Zweig
      prueft den Anbieter nicht). Zusatz-NIT: Selbstheilung haengt am
      Engagement-Client, der Scam-Judge laeuft aber ueber scam_pitch::call_judge.
      Nach d2s Fix: auf 08cc9031+ rebasen, mergen, Test-Gate, deployen, DANACH
      FIREWORKS_MODEL aus ~/.config/deadlock/bots.env entfernen (Backup
      bots.env.bak-fwmodel) und Dienst neu starten. Bis dahin traegt der Env-Override.
      Stand 2026-08-15 15:20: der Env-Wert bleibt
      accounts/fireworks/models/deepseek-v4-flash-0731. Ein zwischenzeitlicher
      Wechsel auf kimi-k3 ist auf Ansage des Nutzers zurueckgenommen: der Bot
      bedient FAQ, Moderation und Concierge, dafuer ist die Flash-Klasse
      richtig, ein Modellwechsel ist keine Agenten-Entscheidung. Der Cold Start
      wird stattdessen ueber das Zeitlimit aufgefangen (Concierge 45 s statt
      8 s, gleichauf mit dem HTTP-Versuch in dl-ai). Der Wert traegt allein.
      dl-ai hat gar keinen Resolver, nur einkompilierte Defaults, die auf das
      tote deepseek-v4-flash zeigen: dl-ai/src/lib.rs DEFAULT_FIREWORKS_MODEL,
      tb-llm/src/selection.rs und tb-engagement/src/crew_review.rs. Wenn der
      Resolver durch ist, gehoert dl-ai denselben Weg zu bekommen, sonst faellt
      der Deadlock-Bot beim naechsten Modellwechsel wieder auf ein totes Modell.
- [ ] Deadlock-Pause-Sweep beobachten: erste Welle nach Deploy (10 Min Anlauf,
      5 Unmods/15 Min), 6 DM-Kanaele + stumme Markierungen im Admin-Log.
      miracleghost9/whysolowkey/pixelpiratemarvin sind echt gebannt; Re-Pausierung
      von miracleghost9 macht der stuendliche Sweep selbst.

## Offen (naechste Sessions)

- [ ] Alarm-Eskalation unit-failure-notify: statt bis zu 10 gleicher
      Discord-Zeilen eine Meldung mit Dauer ("seit X Min in Restart-Schleife")
      und Eskalation; heutige Restart-Schleife wurde gemeldet, ging aber unter.
- [ ] docs/architecture/social-media.md beschreibt noch die toten Python-Pfade;
      auf tb-social-media umschreiben (eigener Doku-Job, Deadlock-Docs-Regeln).
- [ ] Kategorie-Katalog: twitch_game_id per Helix search_category_id aufloesen
      (laeuft aktuell ueber Namensabgleich).
- [ ] Einspruchs-Ansicht fuer den Freigabe-Modus veto_window (plant bislang wie
      Vollautomatik, Widerspruch = Clip vor dem Termin verwerfen).
- [ ] EventSub-403-Signal ("streamt kein Deadlock mehr", 9/15 Kandidaten) als
      Nutzungs-Indikator festgehalten; bewusst noch keine automatische Konsequenz.
- [ ] Aufraeumen nach Freigabe: rust/target/release/tb-bot.engagement-05-15,
      dist.bak-20260814/-20260815 unter bot/analytics/dashboard_v2/, alte
      tb-bot.bak-*. Erst loeschen, wenn der neue Stand ein paar Tage stabil ist.
- [ ] Spaeteres Feature (Entscheidung 2026-08-15): KI-Clip-Editor beim
      Onboarding; Stufe 2 (Video-Verstehen fuer Titel/Keywords, ohne Scoring)
      und Stufe 3 (halbautonomer Lern-Agent) laut Projektseite
      Deadlock-2nd-Brain/projekte/social-media-dashboard.md.

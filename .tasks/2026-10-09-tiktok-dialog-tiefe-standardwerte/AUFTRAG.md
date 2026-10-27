# Auftrag: TikTok-Dialog (Tiefe, Standardwerte, Speichern) und unscharfe Cam

Auftraggeber: Claude-Hauptsession (Orchestrator). Rolle: Blatt-Worker, keine weiteren T3-Threads.
Repo: ~/repos/Deadlock-Twitch-Bot, Basis origin/main (c6e1e6301 oder neuer). Eigener Worktree `~/.worktrees/tb-tiktok-dialog-tiefe`, Branch `feat/tiktok-dialog-tiefe-standardwerte`. Den geteilten Checkout nicht anfassen.

## Nutzerwunsch (mit Screenshots)
Im Social-Media-Manager (`/twitch/social-media`), Dialog "TikTok-Veröffentlichung" (`bot/dashboard_v2/src/components/socialmedia/TikTokPostDialog.tsx`):
1. "Wer darf den Clip sehen?" ist praktisch unsichtbar: das Select hat ohne Fokus keinen sichtbaren Rahmen und keine Tiefe, wirkt wie Fließtext.
2. Die Beschreibungs-Textarea ist nur bei Fokus hervorgehoben, ohne Fokus verschwindet der Kasten. Kästen sollen immer sichtbar sein.
3. Allgemein: Eingabefelder, Selects und Checkboxen haben keine Tiefe. Jedes Bedienelement muss auch ohne Fokus klar als Feld erkennbar sein.
4. Standardeinstellungen: nicht jedes Mal alles manuell setzen, Felder sollen aus den Standardwerten des Streamers kommen.
5. Ein Knopf "Speichern": Beschreibung und Einstellungen eines Clips speichern, ohne ihn schon bei TikTok einzuplanen. Beim nächsten Öffnen des Dialogs steht alles wieder drin.
6. Die Facecam im fertigen Hochkant-Clip ist immer unscharf. Frage des Nutzers: liegt das an der 1080p-Quelle?

## Umsetzung
- Optik zentral in der `studio-input`-Regel und den Checkbox-Stilen des Dashboards lösen (Bestand suchen), damit alle Studio-Felder profitieren. Hausstil: Inputs dunkler und versenkt (warmes Recessed-Schwarz, Innenschatten), dauerhaft sichtbare Goldkante in gedämpfter Stärke, bei Fokus kräftiger. Select mit gut sichtbarem Pfeil. Checkboxen in Gold, gut erkennbar. Nichts darf ineinanderlaufen, kein Glow, keine Pastelltöne.
- Standardwerte, ACHTUNG TikTok-Vorgabe: Die UX-Richtlinien der Content Posting API (Direct Post, relevant fürs App-Audit) verlangen nach unserem Stand Sichtbarkeit ohne vorausgewählten Wert, Interaktionen nicht vorab angehakt und Werbekennzeichnung als bewusste Wahl. Die App ist noch nicht auditiert. Zuerst die aktuelle offizielle Richtlinie prüfen (https://developers.tiktok.com/doc/content-sharing-guidelines, mit Moli, nicht Brave; vorher `~/Documents/claude-config/wissen/agent-browser.md` lesen).
  - Verbietet sie Vorbelegung: Standardwerte je Streamer (Sichtbarkeit, Kommentare/Duett/Stitch, optional Beschreibungsvorlage) serverseitig je Twitch-User-ID speichern (bestehenden Social-Media-Einstellungsspeicher wiederverwenden, kein localStorage; neue Migration nur ohne passenden Speicher) und im Dialog einen deutlichen Knopf "Meine Standardwerte übernehmen" anbieten, plus "Als Standard speichern". So bleibt es eine aktive Nutzerwahl. Werte, die TikTok für das Konto nicht anbietet, beim Übernehmen ignorieren.
  - Erlaubt sie Vorbelegung: direkt vorbelegen, Beleg mit Link im Bericht.
  - Werbekennzeichnung und Musik-Zustimmung nie vorbelegen.
- Speichern (Punkt 5): Entwurf je Clip serverseitig speichern (Beschreibung, Sichtbarkeit, Interaktionen, Werbeangaben), ohne Upload-Warteschlange. Gleiche Richtlinienprüfung wie oben: ein gespeicherter Entwurf des Nutzers ist dessen eigene Eingabe; falls die Richtlinie auch das als Vorauswahl wertet, die Felder per Knopf "Gespeicherten Entwurf laden" setzen. Die Musik-Zustimmung wird nie gespeichert.
- Unscharfe Cam (Punkt 6): Ursache messen statt raten. Den Render-Pfad der Hochkant-Vorschau (Facecam-Ausschnitt, Skalierung, Encoder-Einstellungen, Bitrate) im Code finden und an einem echten Clip prüfen: Größe des Cam-Ausschnitts in der Quelle, Zielgröße im 1080x1920-Bild, Skalierungsfaktor, Skalierfilter (bilinear/bicubic/lanczos), CRF/Bitrate, Mehrfach-Encodes. Hochrechnen eines kleinen Ausschnitts aus 1080p ist eine plausible Ursache, aber nicht die einzige. Befund mit Zahlen. Was sich ohne neue Quelle verbessern lässt (besserer Skalierfilter, Schärfung, kein Doppel-Encode, höhere Bitrate, Twitch-Quelle in höchster Qualität laden) direkt umsetzen und mit Vorher/Nachher-Standbild belegen. Grenzen der Quelle ehrlich benennen.
- Keine Code-Kommentare. Texte in natürlichem Deutsch mit echten Umlauten, keine Em-Dashes, über das i18n-Dictionary wie bisher.

## Prüfen und Abschluss
- Frontend-Build, Lint und bestehende Tests in `bot/dashboard_v2`; bei Rust-Änderung `cargo fmt`, `clippy`, `test` (rustc 1.97, siehe Memory tb-bot-build-toolchain). sqlx-Offline-Dateien mitziehen.
- Sichtprüfung des Dialogs per Moli-Screenshot (Vorher/Nachher), Pfade im Bericht.
- Selbstprüfung mit `gate_hook.py --review`, dann Merge-Gate; bei BLOCK frische Fixer-Subagenten bis ALLOW. Danach Merge nach main in Einzelschritten, Migration falls nötig als postgres, Deploy über `/usr/local/bin/deploy-twitch-release <sha>`, Dienste neu starten, live prüfen, Worktree und Branch löschen, `t3-thread.py settle --selbst`.
- Bericht an den Orchestrator: Branch/SHA, TikTok-Richtlinien-Befund mit Link, Cam-Befund mit Zahlen und Standbildern, Screenshots, Gate-Antwort.

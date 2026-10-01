# Contract: Smalltalk-Test bei Server-Überlast killen

## Ziel
Der Smalltalk-Test (`tb-bot` Smalltalk-Loop, erzeugt die Discord-Karte
"Smalltalk-Testauswertung") beendet bei anhaltender Server-Überlast die aktive
Sitzung und startet keine neue, bis die Last wieder fällt. Der Endgrund ist auf
der Discord-Karte sichtbar. Wiederverwendet wird der bereits vorhandene,
getestete `Lastwaechter` aus `tb-stream-audit`; es entsteht keine zweite
Last-Logik.

## REQ
- REQ1: Ein Last-Monitor im Smalltalk-Loop misst CPU und RAM im Takt (gleiche
  Messung wie `tb-stream-audit`: `/proc/stat` + `/proc/meminfo`, die größere von
  CPU und RAM entscheidet) und füttert einen `Lastwaechter`.
- REQ2: Wird das Gate aktiv (Last ununterbrochen über der Grenze länger als das
  Fenster), wird die aktive Smalltalk-Sitzung mit Endgrund `server_overloaded`
  geschlossen.
- REQ3: Solange das Gate aktiv ist, startet der Loop keine neue Sitzung
  (`process_once` respektiert das Gate).
- REQ4: Die Discord-Karte zeigt für den Endgrund `server_overloaded` das Label
  "Server überlastet". Fehlt das Label, bleibt die Auslieferung fail-closed wie
  bei den anderen Copy-Feldern.
- REQ5: Ist die Auslastung nicht messbar (`/proc` unlesbar), gilt fail-open:
  kein Kill, Gate zurückgesetzt. Grenzwerte/Fenster/Deckel kommen aus derselben
  Env-Konvention wie beim Audit (Standard Grenze 90 %, Freigabe 80 %).

## INV
- INV1: Genau ein `Lastwaechter`-Typ im Repo. Die Entscheidungslogik (Fenster,
  Hysterese, Max-Halte-Deckel) wird nicht kopiert, sondern importiert. Die
  CPU/RAM-Messfunktionen liegen nach dem Umbau an genau einer Stelle und werden
  von `tb-stream-audit` und dem Smalltalk-Loop geteilt.
- INV2: Bestehende Endgründe (`session_timeout`, `stream_ended`,
  `process_start`, `process_shutdown`, `kill_switch`, Provider-Fehler) und das
  Verhalten von `tb-stream-audit` bleiben unverändert.
- INV3: Keine ENV-Datei, keine neuen Secrets. Config nur über normale
  Env-Grenzwerte im bestehenden `STREAM_AUDIT_LOAD_*`-Stil (bzw. konsistent
  benannt, dokumentiert).

## Nicht-Ziele
- Das Provider-Fehler-Problem (74 `generate_error`, 0 erzeugte Nachrichten) wird
  hier NICHT gelöst. Eigener Task.
- KEINE bloße Transkriptions-Drosselung (Pause + Nachholen) für den
  Smalltalk-Test. Der Nutzer hat "Sitzung killen" gewählt.
- `tb-stream-audit` behält sein bisheriges Verhalten (dort weiter drosseln, nicht
  killen).

## Erlaubter Bereich
- `rust/bin/tb-bot/src/smalltalk_loop_wiring.rs` (Last-Monitor, Gate in
  `process_once`, Karten-Copy)
- `rust/crates/tb-stream-audit/src/last.rs` (Messung öffentlich machen / Ort der
  geteilten Messfunktionen) ODER neue Leaf-Crate `tb-load`
- `rust/bin/tb-stream-audit/src/main.rs` (Messung importieren statt lokal halten)
- Betroffene `Cargo.toml` (neue Dependency bzw. neue Leaf-Crate + Workspace)
- `rust/crates/tb-engagement/src/smalltalk_loop_store.rs` nur falls ein
  `close_active_session`-Pfad noch fehlt (er existiert bereits: `close_active_session`).

## Reuse-Entscheidung (verbindlich)
Empfohlen: `Lastwaechter` + die Messfunktionen (`cpu_stand`, `cpu_prozent`,
`ram_prozent`) in eine kleine, reine Leaf-Crate `tb-load` ziehen (nur std +
tracing, kein sqlx/tokio/reqwest). `tb-stream-audit` und `tb-bot` hängen daran.
Grund: sonst zöge `tb-bot` die ganze Audit-Crate (llm/report/sqlx) nur für den
Wächter herein. Fallback, falls die neue Crate zu invasiv ist: `tb-stream-audit`
als Dep von `tb-bot` und die Messfunktionen aus `bin/.../main.rs` nach
`crates/tb-stream-audit/src/last.rs` hochziehen. In beiden Fällen bleibt es EIN
`Lastwaechter`.

## Tests (vor der Implementierung schreiben, erst rot)
- T1: Endgrund `server_overloaded` bildet auf das Karten-Label "Server
  überlastet" ab (analog zu den bestehenden Karten-Tests in
  `smalltalk_loop_wiring.rs`).
- T2: Solange das Gate aktiv ist, startet der Loop keine neue Sitzung; fällt es,
  läuft der Loop normal weiter. Reine Entscheidungslogik testbar halten (Gate als
  Parameter/AtomicBool), ohne echten Server.
- T3: Gate-aktiv-Übergang schließt die aktive Sitzung mit `server_overloaded`
  (Store-Test oder Logik-Test, je nach Schnitt).

## Amendments
(keine)

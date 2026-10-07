# VOD-Archivstatus: Befund

Stand: 2026-10-07, Umsetzung läuft.

BESTAND[BS-1]: teilweise | Fundort: rust/crates/tb-vod-archive/src/store.rs:399 | Anknüpfung: gespeicherte Teilbestätigung, VOD-Abschlusszeit, zentraler YouTubeUploader::video_status und bestehender Archivtab

## Lesender Echtbestand

82 VODs: 81 mit status=archived, eines unavailable. Alle 82 ohne last_attempt_at. Bei 76 archivierten VODs sind uploaded_at und vollständig bestätigte Teile vorhanden. Fünf archivierte VODs haben weder Abschlusszeit noch vollständig bestätigte Teile. Insgesamt 77 done-Teile und drei pending-Teile. Keine Titel, Kanalnamen, IDs oder Geheimnisse aufgenommen.

## Ursache

Die API gibt uploaded_at nicht aus. Das UI deutet fehlendes last_attempt_at stets als „Noch kein Versuch“, obwohl historische Uploadbestätigungen vorliegen. Gesamtstatus steht rechts abgesetzt. Das UI ermittelt Fertigstellung unabhängig vom API-Label allein aus dem VOD-Rohstatus. Widersprüchliche alte Datensätze erhalten dadurch dieselben Fortschritts- und Aktionsregeln wie bestätigte Uploads. Der API-Leser nennt fehlende Bestätigung pauschal einen Fehlschlag, statt unbekannte Historie auszuweisen.

## Bestehender Schreibpfad

YouTube liefert bei erfolgreichem letzten Chunk oder Resume-Abschluss eine Video-ID. Nur diese Übergänge setzen ein Teil auf done. Der VOD-Abschluss speichert uploaded_at, prüft aber bislang die Teile nicht selbst. Der Worker prüft frische Videos über den bestehenden YouTube-Client und korrigiert rejected/failed bzw. fehlende Videos atomar. Vor lokaler Bereinigung prüft derselbe Client uploadStatus=processed. Drive prüft nach copyto jede Dateigröße per lsjson, erstellt den Link und schreibt danach drive_uploaded samt uploaded_at.

Uploadbestätigung ist keine aktuelle Plattform-Verfügbarkeitsgarantie und keine Aussage über Sichtbarkeit. Die Oberfläche wird diese Grenze nennen. Keine zusätzliche Plattformabfrage beim Listenaufruf, kein neuer Dienst und kein manueller Datenpatch.

## Umsetzung

API liefert einen gemeinsamen abgeleiteten Anzeigezustand, bestätigte YouTube-Teile, separate Zielabschlüsse, uploaded_at und erlaubte Wiederholbarkeit. Widersprüchliche historische Abschlüsse bleiben unklar. Rust-Schreibpfad sichert VOD-Abschluss gegen leere/unvollständige Teile ab, speichert Versuche am tatsächlichen Bearbeitungsbeginn und beendet laufende Kennzeichnungen bei Unterbrechung durch Laufgrenzen. UI zeigt Status mit Icon links unter dem Titel, danach Fortschritt, Metadaten/Abschlusszeit, Ziellinks und zurückhaltendes Ausblenden.

## Werkzeuggrenze

ctx_execute_file ist korrekt an den eigenen Worktree gebunden und lehnt /tmp-Logdateien sowie die externe Gate-Quelldatei als außerhalb der Projektwurzel ab. Die Ablehnung wird nicht über einen anderen Lesekanal umgangen. Weitere Prüfungen schreiben ihre Logs in die Taskakte innerhalb des Worktrees; der erste Compiler- und Frontendlauf wird dort wiederholt. Der reguläre Gate wird über seine öffentliche Befehlsoberfläche aufgerufen.

## Vorher-Prozesse

Release 2ead4d556327596fcc7d9feeaceb848e910cf8f8. PIDs: Bot 1807853, Dashboard 1807722, Coaching 1809122, Collector 1809132. Alle exe ohne deleted, NRestarts=0.

# Auftrag D

Intent-Thread: d3a1741e-82bc-4a48-865b-2845c663dca7

B6: fehlende oder unvollständige Verbindungen bereits in der gemeinsamen Queue und im Worker sichtbar vertagen. B7: Plattformfähigkeiten ausweisen, keine TikTok-Statistikversuche. B8: bei Sammelverbindung eigenes Konto verbinden; eigene Verbindung löschen und beim Anbieter widerrufen, soweit unterstützt. C2: tatsächliche YouTube-Sichtbarkeit speichern und anzeigen. C4: Löschfristen an laufenden Code anpassen. Anzahl gestarteter Worker korrekt protokollieren.

Aktualisierung vom Haupt-Orchestrator: C1 entfällt. TikTok-Textpassagen bleiben unverändert, Direct Post gehört Auftrag E. Anreicherung gehört Auftrag F und bleibt unangetastet.

Worktree: /home/nathanael/.worktrees/tb-social-golive-upload
Branch: fix/social-golive-upload-texte

Prüfung: Rust-Formatierung, Clippy, betroffene Tests mit echter Testdatenbank, Dashboard-Build, Sichtprüfung. Abschluss: Rebase auf origin/main, lokaler Merge-Gate, bei ALLOW Push HEAD:main, Migration, Release-Deploy, Dienstneustart, Live-Belege und Aufräumen.

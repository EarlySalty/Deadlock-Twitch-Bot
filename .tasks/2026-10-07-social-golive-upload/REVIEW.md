# Lokaler Merge-Gate für Auftrag D

## Prüfgegenstand

Branch `fix/social-golive-upload-texte`, integriert auf frischem `origin/main` e0b0dbaf. Die lokale Referenz `main` ist ein älterer, fremd geänderter Checkoutstand; der Gate-Vergleich verwendet deshalb ausdrücklich `origin/main` und verändert den gemeinsamen Checkout nicht.

Umfang: Queue-Wartezustand und Wiederaufnahme, ehrliche Plattformfähigkeiten, Löschung eigener Kontoverbindungen, tatsächliche YouTube-Sichtbarkeit, Rechtstexte und tatsächliche Worker-Zählung. Auftrag F ist als Basis integriert. Die TikTok-Umstellung von Auftrag E gehört nicht zum Diff.

## Bekannte Bestandsfehler

Die vollständige API-Suite hat auf Basis und Änderungsstand dieselben 22 Fehler bei jeweils 1309 bestandenen Tests. Die zwei paketlokalen Clippy-Befunde sind ebenfalls identisch. Betroffene Social-Media-Handler und die Social-Media-Bibliothek bestehen; genaue Befehle und Grenzen stehen in EVIDENCE.md.

## Gate-Runde 1

Ausführung nach dem Prüfcommit mit `gate_hook.py --review`. Vollständiges Urteil und eventuelle Mängelliste werden unter `/tmp/tb-upload-gate-review.log` erfasst. Bis zum tatsächlichen ALLOW erfolgen weder Push auf main noch Deploy. Bei BLOCK ist ein frischer Fixer nötig; wegen des Blattauftrags werden keine zusätzlichen Threads eigenmächtig gestartet.

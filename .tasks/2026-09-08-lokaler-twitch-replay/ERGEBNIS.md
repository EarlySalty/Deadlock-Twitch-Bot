status: abgeschlossen
datum: 2026-09-08

# Ergebnis des lokalen Twitch-Versuchs

Beide Modelle haben den vollständigen eingefrorenen Datensatz bearbeitet: jeweils 50 personalisierte Fälle und zehn allgemeine Vergleichsantworten zu denselben ersten zehn Fällen. Alle 120 Aufrufe lieferten technisch verwertbare, nicht abgeschnittene Antworten. Beide Runner endeten mit Exit 0 und `complete=true`. Es gab keine Cloudaufrufe, kein Training und keinen Nachrichtenversand.

| Messung | Qwen3.5 4B | Qwen3.5 9B |
|---|---:|---:|
| Gesamtlauf einschließlich zehn Baselines | 63,8 Minuten | 113,4 Minuten |
| Personalisierte Antworten | 50 | 50 |
| Median dieser 50 Antworten | 72,1 Sekunden | 133,2 Sekunden |
| P95 dieser 50 Antworten | 97,4 Sekunden | 171,6 Sekunden |
| Median personalisiert, nur die zehn gepaarten Fälle | 71,2 Sekunden | 125,5 Sekunden |
| Median allgemein, dieselben zehn Fälle | 7,4 Sekunden | 15,6 Sekunden |
| Fehler / leere Antworten / abgeschnitten | 0 / 0 / 0 | 0 / 0 / 0 |

Die Auswertung verwendet die im Runner hinterlegten Rangquantile. Laufzeiten beziehen sich auf diesen konkreten Promptaufbau mit vier CPU-Threads, 8192 Kontext und einem Slot. Der wechselnde Gesprächskontext steht vor Stilbeispielen und Wissen; eine anders angeordnete, besser wiederverwendbare Vorgabe wurde nicht getestet. Diese Zahlen sind keine allgemeine Geschwindigkeitsuntergrenze lokaler Modelle.

Verglichen wird die ganze personalisierte Vorgabe mit ausgewählten älteren Stilbeispielen und vorhandenem Wissen gegenüber derselben Systemkarte und demselben aktuellen Kontext ohne diese beiden Zusätze. Das isoliert die Wirkung des Stils nicht. Temperatur 0,4 und kein fester Seed bedeuten: Inputs, Parameter und Artefakte sind nachprüfbar, identische Antworttexte bei Wiederholung nicht garantiert.

## Greifbare Artefakte

- Direkter privater Vergleich mit historischen Referenzen, 4B und 9B: `/home/nathanael/.local/share/twitch-local-eval/results/qwen3.5-9b-20260908/comparison.html`.
- Vollständige Modellantworten, Kennzahlen und jeweilige Manifeste: die beiden Ordner `results/qwen3.5-4b-20260908/` und `results/qwen3.5-9b-20260908/` in derselben privaten Versuchsablage.
- Geprüft: 60 von 60 identische Prompt-Prüfsummen zwischen beiden Läufen, 50 HTML-Fälle, 170 Antwort-/Referenzkarten, keine Script-Tags oder externen Assets. Nachweis: `runtime/artifact-validation.json`.
- Ordner 0700, private Ergebnisdateien 0600. Das exakt getestete Binary liegt dauerhaft mit privaten Ausführungsrechten 0500 unter `runtime/bin/tb-local-replay-63910dc2bae18cdbcb8a0d8eb63470e4033ec972c07637fdcc607dd5715133e6`. Der SHA-256 wurde nach dem Sichern erneut geprüft.
- Geprüfter und ausgeführter Code-Stand: `70e55a1e1a3c1edae6f171d03657cc296e504584`. Spätere Dokumentations-/Integrationscommits ändern diese Herkunft der Messung nicht.

## Einordnung

Rein lokale Formmessung über dieselben 50 Hauptfälle ohne Ausschlüsse:

| Kennzahl | Historische Referenzen | 4B personalisiert | 9B personalisiert |
|---|---:|---:|---:|
| Median Zeichen | 26 | 61 | 45 |
| Median Wortgruppen | 4 | 10,5 | 7 |
| Mehrzeiler | 0 % | 30 % | 0 % |
| Mit Fragezeichen | 22 % | 10 % | 14 % |

9B liegt bei Länge und Einzeiligkeit näher an den Referenzen, bleibt aber länger. Das ist keine Bewertung von Natürlichkeit, richtigem Deutsch, passenden Fakten oder Konversion. Markdown und sieben vorab festgelegte Floskeln wurden in keiner Gruppe gefunden; das beweist keine natürliche Sprache. Bei denselben zehn gepaarten Fällen verkürzt die gesamte personalisierte Vorgabe die 9B-Antworten im Median von 72,5 auf 59 Zeichen; bei 4B steigt der Median von 65,5 auf 71,5 Zeichen. Die Wirkung des Stils allein ist damit nicht isoliert.

Dauerhafte private Nachweise: `runtime/style-metrics-final.json`, `runtime/style-metrics-final.md` und `runtime/style-metrics-method.md`, jeweils 0600. Der Pool mit 16 älteren Vorlagen hat selbst 46 Zeichen und 8,5 Wortgruppen im Median. Eine repräsentativere Auswahl ist ein konkreter Folgehebel; weder seine Wirkung noch eine andere Promptanordnung wurden in diesem eingefrorenen Versuch getestet.

Der private Vergleich macht tatsächliche Antworten prüfbar. Die technischen Erfolge sind keine Freigabe für natürliches Deutsch, Nanis Stimme oder autonomes Streamer-Onboarding. Der kleine Pool aus 16 älteren Vorlagen hat selbst längere Texte als die 50 Referenzen; diese Datenbegrenzung und die zusätzliche lokale Stilmessung gehören zur Interpretation. Es gab keine menschliche Qualitätsbewertung der Antworten.

Das Feature bleibt standardmäßig aus. DeepSeek bleibt unverändert das produktive Modell. Nanis Stil gilt ausschließlich für Twitch, nicht für den Concierge. Produktives Anschließen des Kontextaufbaus ist eine eigene Folgearbeit und wurde durch diesen Offlinevergleich nicht erledigt.

Beide lokalen Modell-Units wurden nach dem Versuch gestoppt; Port 18789 ist frei. Bot und Dashboard blieben aktiv, ohne Neustart oder Speicherlimitverletzung. Die CPU-Messung stammt vom gleichzeitig produktiv genutzten Host. Der private Abschlussnachweis liegt unter `runtime/replay-stop-status.txt`, die vollständigen technischen Zahlen unter `runtime/9b-replay-technical-summary.json`.

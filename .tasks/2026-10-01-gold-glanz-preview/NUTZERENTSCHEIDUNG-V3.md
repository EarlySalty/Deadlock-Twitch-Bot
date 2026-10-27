# Nutzerentscheidung V3

Der Nutzer hat die kräftigere Variante Poliertes Gold (Revision 3) angenommen:

> Besser ja das können wir erstmal nehmen

Gewählte Gestaltung: gold=polished, ruhiger Verlauf ohne Streifen, drei weich ineinander laufende Goldtöne und feine Innenkante. Implementierung b597ea01; geprüfter Vorschau-Stand 7cf9ea75.

Die Entscheidung geht an den Intent-Thread 37ae96e4-49e9-46b7-a77f-e148784352c1. Dieses Paket bleibt auf Vorschau begrenzt; kein Merge oder Deployment durch diesen Thread.

## Verbindliche Stilgrenze

Nach der Farbentscheidung präzisiert der Nutzer:

> nur halt mit dem Style aufpassen du verwendest ein anderes Dashboard unser Dashboard sieht Stylich anders aus das du nur die Farbe abänderst

Angenommen ist ausschließlich die Goldfarbe beziehungsweise der ruhige Goldverlauf von Poliertes Gold V3. Das bestehende tatsächliche Dashboard ist die Grundlage: Layout, Abstände, Größen, Schriften, Formen und Komponentenstyling bleiben erhalten. Die Vorschau ist eine Farbreferenz, keine Freigabe ihres gesamten Erscheinungsbildes.

Diff-Prüfung gegen Basis 14bc1f47: index.css und DashboardShell.tsx unverändert. Die Vorschau enthält zusätzlich Demo-Daten, lokal ergänzte Schriftdateien, Vorschau-Routing, Backend-Sperre und erzwungene Sichtbarkeit von Sidebar/Hilfe. Diese Vorschau-Infrastruktur darf nicht als Styling-/Layoutänderung ins tatsächliche Dashboard übernommen werden. Auch Oberflächen-/Schatten-/Text-Kontrasteingriffe aus gold.css müssen beim Übertragen auf den ausdrücklich beauftragten Farbwechsel begrenzt werden. Kein pauschaler Merge des Vorschau-Branches; im Folgeauftrag ausschließlich die ausgewählte Farbe auf den aktuellen tatsächlichen Dashboard-Stand übertragen.

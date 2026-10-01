# Sicht- und Codeprüfung

Selbstprüfung im einzigen beauftragten Paketthread.

- Vorschau-Aktivierung ist auf import.meta.env.MODE === preview begrenzt. Original-Ansicht erhält kein data-gold-Attribut.
- Status- und Chart-Tokens werden nicht ersetzt. CSS adressiert Akzentflächen, nicht Diagrammpfade oder Seitenhintergründe.
- Goldschatten sind ausschließlich inset; Avatar bleibt beim vorhandenen neutralen Ring. Der bisherige Goldschatten am Header-Icon wird nur in den Goldvarianten entfernt.
- Preview-Fixtures erhalten die für Analyse und Home benötigten Daten. Bereits normalisierte Home-Fixtures werden nicht erneut durch den Produktionsmapper geleitet.
- Preview-Build darf Demo-Fixtures auf dem Tailscale-Host benutzen. Backend-Aufrufe werden im Preview blockiert.
- Tests für Demo-Sidebar und Hilfe erlauben die ausdrücklich beauftragte lokale Vorschau. Neue Metallfarben sind im Palettentest ausschließlich für preview/gold.css freigegeben.
- Markentokens in index.css und Diagramm-Komponenten unverändert. Kein Merge-Gate, Merge oder Deployment durchgeführt.

Revision 2: Lichtkamm und Champagner entfernt. Die Verläufe laufen gleichmäßig von einem helleren zu einem dunkleren Goldton, jeweils mit drei weit verteilten Farbstufen. Neue Screenshots in screenshots-v2/ ersetzen für die Entscheidung den alten Satz.

Revision 3: Sichtprüfung der neuen polierten Flächen bestätigt den etwas kräftigeren Goldton und den weiterhin ruhigen Verlauf ohne Bänder. Neue Aufnahmen in screenshots-v3/; die vorherige Poliert-Fassung bleibt als screenshots-v2/ zum Vergleich erhalten.

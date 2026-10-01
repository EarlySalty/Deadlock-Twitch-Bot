# Sicht- und Codeprüfung

Selbstprüfung im einzigen beauftragten Paketthread.

- Vorschau-Aktivierung ist auf import.meta.env.MODE === preview begrenzt. Original-Ansicht erhält kein data-gold-Attribut.
- Status- und Chart-Tokens werden nicht ersetzt. CSS adressiert Akzentflächen, nicht Diagrammpfade oder Seitenhintergründe.
- Goldschatten sind ausschließlich inset; Avatar bleibt beim vorhandenen neutralen Ring. Der bisherige Goldschatten am Header-Icon wird nur in den Goldvarianten entfernt.
- Preview-Fixtures erhalten die für Analyse und Home benötigten Daten. Bereits normalisierte Home-Fixtures werden nicht erneut durch den Produktionsmapper geleitet.
- Preview-Build darf Demo-Fixtures auf dem Tailscale-Host benutzen. Backend-Aufrufe werden im Preview blockiert.
- Tests für Demo-Sidebar und Hilfe erlauben die ausdrücklich beauftragte lokale Vorschau. Neue Metallfarben sind im Palettentest ausschließlich für preview/gold.css freigegeben.
- Markentokens in index.css und Diagramm-Komponenten unverändert. Kein Merge-Gate, Merge oder Deployment durchgeführt.

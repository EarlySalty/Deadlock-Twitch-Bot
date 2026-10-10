# TikTok-Musikzustimmung im Standard

Auftrag: Bewusste Musikzustimmung beim Speichern der Standardwerte ermöglichen. Keine Zustimmung aus Entwürfen. Markenpartner brauchen weiterhin eine Zustimmung je Clip. Gespeicherte Zustimmung ist an das TikTok-Konto gebunden und durch erneutes Speichern ohne Haken widerrufbar.

Basis: origin/main 9ff9d2b3c. Blatt-Worker, keine weiteren T3-Threads.

## Umsetzung

1. Rust speichert music_consent, serverseitigen Zeitpunkt und Konto-ID ausschließlich für Standardwerte. Beim Lesen wird die Konto-ID mit der aktuellen kanalgebundenen TikTok-Verbindung verglichen.
2. Der Dialog bietet einen separaten, anfangs leeren Haken beim Standard. Übernommene Zustimmung bleibt bei Text-, Sichtbarkeits- und Interaktionsänderungen aktiv. Der Veröffentlichungshinweis bleibt sichtbar.
3. Betroffene Rust-Tests, Formatprüfung, Clippy, Frontend-Build und Lint ausführen. Moli für Sichtprüfung verwenden. Gate, Merge, Deploy, Live-Prüfung und Bereinigung abschließen.

Keine Kommentare hinzufügen. Keine Python-Anwendungsänderung. Kein Eingriff in den geteilten Checkout.

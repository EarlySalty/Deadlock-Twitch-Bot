# Nachtrag 2 (2026-09-18): Der Bot kommt mit jeder Twitch-Einstellung klar

Wunsch des Nutzers: egal, was der Streamer bei Twitch eingestellt hat, der Bot erkennt es und macht das Beste daraus. Grenze: Helix kann den Twitch-Werbeplan nur lesen, pausieren und Werbung starten, die Werbemenge bei Twitch selbst kann kein Bot ändern.

## Paket A

- Aus dem gelesenen Plan (Länge, Abstand, verfügbare Pausen) ableiten, ob er schützbar ist: passt die geplante Werbung mit Vorziehen und den vorhandenen Pausen in Fenster, oder läuft sie zwangsläufig ins Match. Maß dafür sind die echten Match- und Queue-Längen des Kanals aus den Steam-Daten, nicht geratene Werte.
- Der Bot passt sein Vorgehen an den Plan an: bei dichtem Plan Pausen nur für die wertvollsten Momente ausgeben (Match-Mitte, Raid) statt sie früh zu verbrauchen, bei lockerem Plan konsequent vorziehen.
- Status bekommt `plan.fit: 'good' | 'tight' | 'unprotectable'` und `plan.suggestion` (empfohlene Werbeminuten pro Stunde und Blocklänge für die Twitch-Einstellung, oder null). Die Empfehlung kommt aus den Kanaldaten; solange Paket C keine Wirkungsdaten liefert, aus Match- und Queue-Längen.
- Zustandswechsel von `fit` einmal in den Verlauf schreiben, nicht bei jedem Tick.

## Paket B

- Bei `tight` oder `unprotectable` eine ruhige Hinweiszeile in der Status-Karte: was der Bot trotzdem tut, und der konkrete Vorschlag mit Link zum Twitch-Werbungs-Manager (`https://dashboard.twitch.tv/monetization/ads/ads-manager`). Kein Warnbalken, keine Pflicht, bei `good` nichts anzeigen.

# Evidence: Partner-Übersicht Sortierung

status: aktiv
datum: 2026-09-04

Jede Zeile eine echte Fundstelle, Stand main 1540aebb.

- website/src/hooks/useNetworkStreamers.ts:6-15: Typ `NetworkStreamer` mit `isLive`, `viewers`, `game`, `dlStreams30d`, `avgViewers30d`; alle Felder für Deadlock-Filter und Impact-Wert vorhanden.
- website/src/hooks/useNetworkStreamers.ts:43-54: `sortStreamers` sortiert heute live zuerst nach Zuschauern, offline nach `dlStreams30d`, dann Name; hier oder in `lib/partnerNetwork.ts` kommt die Impact-Sortierung hin.
- website/src/hooks/useNetworkStreamers.ts:64-83: ein Fetch mit `cancelled`-Guard, Status loading/ready/error; bleibt (INV-04).
- website/src/components/partner-clean/PartnerNetwork.tsx:14-30: `TwitchEmbed` (player.twitch.tv, parent, muted, autoplay).
- website/src/components/partner-clean/PartnerNetwork.tsx:32-52: `LivePreview` mit Vorschaubild und Avatar-Fallback; wird für REQ-01 nicht mehr gebraucht, Live-ab-Platz-4 wandert in die Liste (REQ-02).
- website/src/components/partner-clean/PartnerNetwork.tsx:107: `OfflineTile` (Avatar, Name, "N Deadlock-Streams in 30 Tagen"); Vorlage für die Listenzeilen.
- website/src/components/partner-clean/PartnerNetwork.tsx:176-179: Aufteilung `live`/`offline`, `embedded = live.slice(0,3)`, `previewed = live.slice(3)`; hier greift der Deadlock-Filter.
- website/src/components/partner-clean/PartnerNetwork.tsx:194-200: Zähler `{streamers.length} Partner` und Einleitungssatz (REQ-05 bleibt).
- website/src/components/partner-clean/PartnerNetwork.tsx:223-232: Block "Gerade offline" mit vollem Raster; wird durch die Ausklappliste "Alle Partner" ersetzt.
- website/src/lib/partnerNetwork.ts:22-35: `twitchUrl`, `twitchParent`, `previewImageUrl`; Ablage für reine Funktionen wie `impactScore` und `istDeadlock`.
- rust/crates/tb-dashboard-api/src/handlers/network.rs:29-32: `game` ist `Option<String>`, laut Doku nur bei Live-Kanälen mit echter Kategorie gesetzt; Test network.rs:424 erwartet exakt "Deadlock".
- website/tests/partnerPage.test.mjs:17-25: FORBIDDEN-Liste; bleibt scharf (REQ-07).

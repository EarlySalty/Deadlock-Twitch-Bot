# Review: Partnernetz-Sortierung /streamer/v2

status: erledigt
datum: 2026-09-04
ausgang: FREIGABE

WIRKUNGSPRUEFUNG[WP-1]: 1 Befund (minor) | Zwillingssuche: grep-belegt (istDeadlock nur in partnerNetwork.ts, game-Anzeige nur ChannelBar und PartnerZeile) | Fremddienst-Pfade: 0/0 (Fetch in useNetworkStreamers unveraendert, ausser Scope)

Logik korrekt: istDeadlock (nur game, case-insensitiv, undefined bzw. {} ergibt false), impactScore (50/50, Maximum 0 ergibt 0, kein NaN), gliederePartner (embeds max 3 Deadlock-Live nach Viewern, weitereDeadlock Rest, allePartner nach Impact, Gleichstand Name; Dedup ueber login, Summe gleich Eingabe, kein Verlust, keine Doppelung). TSX: aria-expanded, Standard eingeklappt, Raster 1/2/4, Leerzustand KEIN_DEADLOCK_TEXT, Fehler-Leerzustand bleibt, Zaehler streamers.length, Links target=_blank rel=noopener noreferrer, Nicht-Deadlock in "Alle Partner" via deadlockLive={false} ohne LIVE-Punkt und ohne Spielname (Amendment erfuellt). LivePreview lebt als Reduced-Motion-Ersatz fuer das Autoplay-Embed (asPreview=Boolean(reduce)), kein toter Code.

Scope sauber: nur PartnerNetwork.tsx, lib/partnerNetwork.ts, tests/, .tasks/ (git stat bestaetigt). Hero, PartnerPitch, StreamerNetworkPage, v1, useNetworkStreamers, partnerShared.tsx nicht im Diff, byteidentisch. Intro-Satz und ChannelBar-game-Anzeige stammen aus origin, unveraendert.

Tests: node --test tests/*.test.mjs ergibt 35/35 gruen, neue Datei 5/5; tsc --noEmit exit 0. Scharf: embeds-deepEqual ["charlie","alpha","bravo"] wuerde bei Rueckbau des istDeadlock-Filters rot (foxtrot mit 900 Viewer WARDOGS wandert sonst nach embeds[0]); foxtrot explizit in allePartner geprueft. Texte: keine Em-Dashes, kein ae/oe/ue-Ersatz in Prosa, kein SaaS-Vokabular, keine Nummerierung.

## Mangelliste

1. website/tests/partnerNetzSortierung.test.mjs:46 (minor): Die Anforderung "ohne LIVE-Markierung" aus dem Amendment ist nicht getestet. Dass foxtrot in "Alle Partner" ohne LiveDot und ohne Spielname rendert, haengt allein am JSX-Literal deadlockLive={false} in PartnerNetwork.tsx:290 und bleibt ungeprueft. Code ist korrekt, nur die Testabdeckung fehlt. Optionaler Fix: Assertion, dass die allePartner-Map deadlockLive={false} setzt (Quelltext-Match analog zum aria-expanded-Test). Kein Merge-Blocker.

# Evidence: Spam-Lernen für Angebot-plus-Domain und generische Einzelwörter

Stand 2026-09-07, Live-Checkout main 83f069b4.

## Vorfall 1: "discord" als gelerntes Fragment

- `twitch_auto_learned_spam_patterns` id 9: pattern `discord`, type `fragment`, Kanal luckymakkydead, 2026-09-05 21:55 UTC, Quelle "I'm on the road right now ... Add me on Discord", Begründung vom Richter (Spalte minimax_reasoning).
- Lernpfad des Richters: `rust/crates/tb-chat/src/scam_pitch.rs:1851` `learn_pattern_from_judge`, Gate `is_distinctive_spam_pattern` (`rust/crates/tb-chat/src/scam_pitch.rs:1897`).
- Gate lässt "discord" durch: Einzelwort ab 6 Zeichen ohne Punkt zählt als Dienstwort, `rust/crates/tb-chat/src/spam_filter.rs:1014` `ist_dienstwort`; Generik-Liste `rust/crates/tb-chat/src/spam_filter.rs:909` `GENERIC_PATTERN_TOKENS` enthält twitch, stream, chat, aber kein discord.
- Jeder `Learned-`-Grund ist hart: `rust/crates/tb-chat/src/spam_filter.rs:477` `has_hard_spam_signal`.
- Score 1 plus hart ergibt `DeleteOnly`: `rust/crates/tb-chat/src/spam_filter.rs:777`.
- Löschung vor dem Richter-Urteil: `rust/crates/tb-chat/src/pipeline.rs:1474` bis 1492 (`already_deleted`), Karte meldet "Nachricht gelöscht (hartes Signal)" `rust/crates/tb-chat/src/pipeline.rs:1630`.
- Ausnahmen nur Mod/Broadcaster und zwei feste Konten: `rust/crates/tb-chat/src/pipeline.rs:1659`, `rust/crates/tb-chat/src/safe_list.rs:40`.
- Beim Laden greift das Gate erneut (`rust/crates/tb-chat/src/spam_filter.rs:625`), ein Eintrag in der Generik-Liste verwirft den Altbestand also ohne DB-Eingriff.
- Live-Beleg: Karten vom 06.09. 23:38 und 23:39 im Kanal arslanfps, Chatter earlysalty (ID 1186925760), Regel-Treffer "Score 1: Learned-Fragment: discord", AI-Urteil harmlos, Nachricht trotzdem gelöscht.

## Vorfall 2: "Ai viewers twitch .ad" wird nie gelernt

- Live-Beleg: Karte vom 07.09. 09:26 im Kanal heavyandgrey, Chatter airtightlingocfcch3h (ID 1535848825), Regel-Treffer "Score 1: Muster: viewer + name", AI-Urteil Spam, Aktion "Nachricht gelöscht + 24h Timeout", Zeile "Nicht gelernt: ai viewers twitch .ad (zu generisch, Distinktivitäts-Gate)".
- Ablehnung: `rust/crates/tb-chat/src/spam_filter.rs:1044` `is_distinctive_spam_pattern`: `ist_dienstdomain` verwirft "twitch.ad", weil das Label "twitch" generisch ist (`spam_filter.rs:1007`); Dienstwort-Zweig greift nur bis `STRICT_MAX_TOKENS = 2` (`spam_filter.rs:979`), das Muster hat vier Tokens.
- Karten-Text "Nicht gelernt": `rust/crates/tb-chat/src/pipeline.rs:434`.
- Richter-Timeout statt Ban ist Absicht: `rust/crates/tb-chat/src/pipeline.rs:216` bis 218 (`AI_SPAM_TIMEOUT_SECS`, reversibel wegen Fehlurteilen).
- Regelpfad, der bei gelerntem Muster den Ban ergibt: Learned-Phrase +2 (`spam_filter.rs:875`), Eskalatoren Kontoalter unter 90 Tagen +1 und Erstnachricht +1 nur bei hartem Signal unter der Schwelle (`spam_filter.rs:762` bis 774), `SPAM_MIN_MATCHES = 3` (`spam_filter.rs:42`).
- Ganzphrasen-Matching für `phrase`: Klartext-contains oder Kompaktform ab 4 Zeichen (`spam_filter.rs:849` bis 858); Kompaktform von "Ai viewers twitch .ad (no space)" enthält "aiviewerstwitchad".
- Kompakt-Falle bei Einzel-Domain: ein Fragment "twitch.ad" (Kompakt "twitchad") würde "die twitch ads nerven" treffen; deshalb Ganzphrase statt Domain-Fragment.
- DB `tb_chat_autoban_log`, letzte 30 Tage, Inhalt mit "viewers": 65 Bans per Regelpfad (spam), 10 Timeouts per Richter, keine Wiederholer je Konto. Domains nach "ai viewers": streamboo . com 40, twitchmax .com 17, streamboo. com 7, twitch .ad (no space) 6, twitchstar .com 2. Nur "twitch .ad" ist ungelernt.
- Gelernte Muster gesamt: 7 Fragmente, 2 Phrasen (`twitch_auto_learned_spam_patterns`), 10 Safe-Muster.

## Bestehende Bausteine

- Mensch-Lernpfad: `rust/crates/tb-internal-api/src/handlers/spam_learning.rs:186` `learn_handler`, Gate `is_distinctive_spam_pattern_vom_menschen` (`spam_filter.rs:1092`).
- Zweiter Richter-Aufrufer: `rust/crates/tb-chat/src/conversation_scam.rs:826`.
- Tests zum Gate: `rust/crates/tb-chat/src/spam_filter.rs:1517` (`ai viewers` ist nicht distinktiv), Richter-Lerntests `rust/crates/tb-chat/src/scam_pitch.rs:2756` und 2790.
- Etablierte Chatter gibt es nur je Kanal und nur im Invite-Pfad: `rust/crates/tb-chat/src/sus_invite.rs:73`, 227.

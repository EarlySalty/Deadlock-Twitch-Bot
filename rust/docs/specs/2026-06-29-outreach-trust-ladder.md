# Spec: Outreach „Trust-Leiter" — erzählender Mehrstufen-Funnel

Status: ENTWURF (User-abgenommen im Grillme 2026-06-29) — Implementierung an Codex
Repo: Deadlock-Twitch-Bot (Rust)

## 1. Ziel (das WAHRE übergeordnete Ziel)

Kalte deutsche Deadlock-Streamer über Zeit von „wer sind die?" zu „geile Community, da will ich mitmachen" bringen — **ohne Druck, ohne Spam, ohne Ban**. Konvertierungsziel: Streamer landet auf der **Website** (primärer Pitch) und/oder im **Discord** (Community), perspektivisch Partner.

Heute: pro Outreach-Moment **eine** dünne Nachricht. Neu: **ein Häppchen pro Raid**, die Story wächst über die Raids hinweg — Neugier → wer wir sind → echte Leute → Einladung → Beziehung → konkreter Wert → frech-neckend.

## 2. Kernprinzipien (nicht verhandelbar)

1. **Kein kalter Kontakt.** Jede Outreach-Nachricht reitet auf einem Raid (der Raid ist der glaubwürdige Anlass). Der bestehende kalte `build_outreach_message`-Pfad wird stillgelegt.
2. **Ein Häppchen pro Raid.** Keine mehrzeiligen Bursts in fremden Kanälen (Spam-/Ban-Signal). Die „Häppchen" sind über die Raids verteilt, nicht über den Moment.
3. **Stufe = Raid-Zähler pro Ziel.** Die Nachricht ergibt sich aus der Anzahl Support-Raids, die wir diesem Ziel bereits geschickt haben.
4. **Vollautomatisches Targeting, keine Interessen-Erkennung.** Passende Ziele werden geraidet, Tempo = normale Raid-Frequenz (kein künstliches Throttling pro Ziel nötig).
5. **Druck steigt nie**, am Ende wird er sogar frech-locker. Warm, menschlich, „wir"-Form, kein AI-Listen-Stakkato, korrekte Umlaute.
6. **Website primär, Discord sekundär.** Beide Links leben im Bot-**Profil/Panels** (AutoMod bannt rohe Links im Chat). CTA im Text immer „… im Profil" / „auf unserer Website (Link im Profil)".

## 3. Die Leiter (Stufe → Nachricht)

`stage = total_support_raids_an_dieses_ziel` (1-basiert). Pools rotieren deterministisch über den Index (z. B. `pool[stage % pool.len()]`), **kein** `rand`/`Date::now`.

| Stufe | Aufgabe | Auswahl |
|------|---------|---------|
| 1 | Neugier + entwaffnen | fester Text R1 |
| 2 | Wer wir sind | fester Text R2 |
| 3 | Echte Leute | fester Text R3 |
| 4 | Sanfte Einladung (Website/Discord) | fester Text R4 |
| 5–6 | Lockere Beziehung | Pool LIGHT (rotieren) |
| 7–10 | Tiefer Pitch (Turniere/Coaching/Events/2.4k) | Pool PITCH (rotieren) |
| 11+ | Frech, mit sichtbarem Zähler `{n}` (endlos) | Pool CHEEKY (rotieren), `{n}` = echter Gesamt-Raid-Zähler |

### Finale Texte (VERBATIM übernehmen — Claude-autoritativ, Codex ändert kein Wort, keine Umlaute kaputt machen)

**Stufe 1:**
> Hey! Wir bringen dir gerade ein bisschen Unterstützung aus der Deutschen Deadlock Community 💜 Wir wünschen dir noch nen geilen Stream! Und falls du öfter mal Support bekommen möchtest, schau gerne bei uns im Profil vorbei. (Keine Sorge, wir sind kein Scam oder sowas. 😅)

**Stufe 2:**
> Und wieder ein bisschen Support für dich 👋 Falls du uns noch nicht kennst: Wir sind die größte und aktivste Deutsche Deadlock Community. Viel Spaß weiterhin!

**Stufe 3:**
> Schon wieder wir 😄 Bei uns sind echte Leute, echte Streamer, die Deadlock genauso lieben wie du. Schön, dich dabei zu haben — viel Spaß weiterhin!

**Stufe 4:**
> Nächste Ladung Support für dich 💜 Wenn du Bock hast, dauerhaft dabei zu sein — alles dazu findest du auf unserer Website (Link im Profil), und unser Discord ist auch da. Kein Stress, schau einfach mal rein. Weiter so!

**Pool LIGHT (Stufe 5–6):**
> - Wieder wir 👋 Hau rein und viel Spaß beim Stream!
> - Kleiner Support von der Deutschen Deadlock Community 💜 Schön, dich regelmäßig zu sehen!
> - Und wieder ein paar Leute für dich 😄 Wir halten die deutsche Deadlock-Szene zusammen am Leben — freut uns, dass du dabei bist!

**Pool PITCH (Stufe 7–10):**
> - Falls du mehr willst als nur Viewer: Bei uns gibt's Turniere, Coaching und regelmäßige Events, über 2.400 Leute sind dabei. Alles dazu auf unserer Website (Link im Profil), Discord auch 💜
> - Wir machen für die deutsche Deadlock-Szene richtig was — Turniere, Coaching, Events, 2.400+ Mitglieder. Wenn du Bock hast mitzumachen, schau auf unsere Website (im Profil)!
> - Du kriegst hier nicht nur Viewer: Turniere, Coaching und 'ne richtig aktive Community warten auf dich. Mehr auf unserer Website (Profil), bis dahin viel Spaß! 💜

**Pool CHEEKY (Stufe 11+, `{n}` = echter Gesamt-Raid-Zähler, endlos):**
> - Raid Nr. {n} 💀 Du genießt unseren Support echt gern, was? 😄 Aber Teil der Community werden willst du nicht? Komm schon — Website im Profil, Discord auch!
> - Und täglich grüßt der Support 😏 Das ist Raid #{n} für dich. Wie oft willst du eigentlich noch Viewer abgreifen, bevor du mal vorbeischaust? Website + Discord im Profil 💜
> - Raid #{n} und immer noch nicht dabei? Langsam wird's persönlich 😂 Alles zum Mitmachen auf unserer Website (Profil). Den Support gibt's natürlich trotzdem weiter 👋
> - Nr. {n} 🫡 Ehre für die Treue — aber so langsam könntest du auch mal offiziell mitmachen 😅 Website im Profil!

## 4. State-Modell

- Stufe leitet sich aus dem **bestehenden** `total_recruitment_raid_count` ab (bzw. dem äquivalenten Pro-Ziel-Zähler in `twitch_partner_outreach` / Recruitment-State). Falls nötig: Zähler-Spalte ergänzen, aber zuerst prüfen, ob `total_recruitment_raid_count` bereits monoton pro Ziel hochgezählt wird — dann wiederverwenden.
- `message_variant_for` (recruitment_messaging.rs:128) wird von `clamp(1,10)` auf das volle Leiter-Modell erweitert (1–4 fest, 5–6 LIGHT, 7–10 PITCH, 11+ CHEEKY mit `{n}`).
- Rotation deterministisch über den Zähler.

## 5. Zu ändernde Code-Stellen (verifiziert 2026-06-29)

1. **`crates/tb-raid/src/recruitment_messaging.rs`**
   - `message_variant_for` / Varianten-Enum + Texte auf die neue Leiter umbauen (Texte aus §3).
   - `max_recruitment_messages = 50` (Z. 36): so anheben/entfernen, dass 11+ **endlos** läuft (kein hartes Cap). Sicherheits-Cap nur noch über Ban/Opt-out/Tages-Limit.
   - `recent_raid_count`-Gate (Z. ~204, `recent_raid_threshold`): so lockern, dass die Leiter mehrfach pro Ziel feuern darf (heute blockt es ab >2 kürzlichen Raids). Tages-Cap bleibt.
2. **`crates/tb-raid/src/external_recruitment_store.rs`**
   - `EXTERNAL_RECRUITMENT_RAID_LIMIT` (Z. 76): die `≥Limit → Blacklist`-Logik darf die Leiter **nicht** bei Stufe ~4 abwürgen. Entweder Limit deutlich anheben oder diese „zu-oft-geraidet"-Blacklist für Leiter-Ziele deaktivieren. **Ban-Blacklist (`twitch_raid_blacklist`) bleibt unangetastet.**
3. **`bin/tb-bot/src/partner_recruit.rs`**
   - Kalten Erstkontakt (`build_outreach_message` + zugehöriger Chat-Send, Z. 41/192) **stilllegen**. Entdeckte Kandidaten stattdessen als Outreach-/Boost-Raid-Ziel einreihen (siehe `outreach_boost.rs` / `target_resolution.rs`), damit sie bei ihrem ersten Raid in Stufe 1 starten. `RECRUIT_COOLDOWN_DAYS`-Logik nur noch für die Ziel-Auswahl, nicht mehr für Kaltnachrichten.
4. **Versand/Wiring** (`bin/tb-bot/src/raid_arrival_wiring.rs`): bei jedem Outreach-Raid die Stufen-Nachricht senden; `{n}` mit echtem Zähler füllen.

## 6. Stop-Bedingungen (überstimmen die Leiter)

1. **Bot im Kanal gebannt** → interne Liste, permanent raus. Nutzt bestehende `schedule_bot_ban_check` → `twitch_raid_blacklist`. **Prüfung an ZWEI Stellen: Raid-Auswahl UND unmittelbar vor Versand.**
2. **Opt-out / Suppression** (OutboundSuppressionStore) → respektieren (existiert).
3. **Partner geworden** → wechselt auf Partner-Track, keine Onboarding-Nachrichten mehr.

## 7. Sicherheits-Rails (MÜSSEN bleiben)

- Tages-Cap `RECRUIT_MAX_PER_DAY = 8` und Throttle bleiben (Schutz Bot-Reputation).
- Keine rohen Links im Chat (AutoMod). CTAs immer „im Profil".
- Ban-Erkennung + Ban-Blacklist bleiben vollständig erhalten.

## 8. Prerequisite (kein Code, User-Aufgabe)

Bot-Profil/Panels müssen **Website-Link (primär) + Discord-Link** enthalten, sonst laufen alle CTAs ins Leere.

## 9. Definition of Done

- [ ] Kalter Erstkontakt-Send entfernt; entdeckte Kandidaten laufen über Raid-Ladder.
- [ ] Leiter 1→4→5/6→7–10→11+ mit verbatim-Texten aus §3; `{n}` korrekt befüllt; Rotation deterministisch.
- [ ] 11+ läuft endlos (kein `max_recruitment_messages`-Abbruch); `EXTERNAL_RECRUITMENT_RAID_LIMIT` würgt nicht ab.
- [ ] Ban-/Opt-out-/Partner-Stops greifen; Blacklist an Auswahl + Versand geprüft.
- [ ] Tages-Cap + No-Link-Regel intakt.
- [ ] `cargo build --release` grün, `cargo clippy` grün, `cargo test` grün (inkl. neuer Tests: Stufenauswahl 1→11+, `{n}`-Einsetzung, Stop-Bedingungen, kein Kalt-Send mehr).
- [ ] Keine kaputten Umlaute in den Texten (grep-Check ä/ö/ü/💜 im Binary/Quelltext).

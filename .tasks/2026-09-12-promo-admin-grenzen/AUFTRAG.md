# Admin-Grenzen für Community-Werbung

## Nutzer-Intent

Der Nutzer beanstandet drei Werbenachrichten in dach_lock um 20:08, 20:29 und 20:49 bei fast ruhigem Chat. Er hat ausdrücklich klargestellt: keine Sonderlogiken für dach_lock; exakt die eingestellten Logiken gelten. Der Chat darf nicht durch zeitgesteuerte Werbung zugespammt werden. Auch der Timer muss die eingestellten Chat-Grenzen beachten.

Gespeicherte Community-Werte: Gesamt-Abstand 20 Minuten, Aktivitätsabstand 20 bis 30 Minuten, mindestens 8 Nachrichten, 2 neue Chatter, Versuchsabstand 5 Minuten, Zuschauerzuwachs 20 Minuten, persönliche Einladungen 5 Minuten und höchstens 6 je Stream. Eigene Kanalwerte bleiben erhalten; keine Angleichung an Standardwerte.

## Verantwortung

Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen. Du bist nicht allein im Codebase. Fremde Änderungen niemals zurücknehmen.

Intent-Thread-ID: de7d1570-c98a-44a8-8dcf-e161568e5020; koordinierende Codex-Session /root, Umsetzung wird durch /root/timer_diagnose überwacht.

Arbeite ausschließlich in /home/nathanael/.worktrees/tb-promo-admin-grenzen, Branch fix/promo-admin-grenzen, Basis main 27d9482b. Ownership: rust/crates/tb-chat/src/promos.rs, unmittelbar benötigte gemeinsame Timerlogik, irreführender Erklärungstext bot/admin_dashboard/src/pages/content/PromoTimers.tsx und notwendige bestehende Tests. Keine kosmetischen Nebenthemen, keine Secrets oder ENV lesen/ausgeben, keine neuen Modelle in Bot/Diensten.

## Belegte Stellen

- promos.rs:1652 overall_ready && (community_channel || activity_ready) umgeht die Chat-Grenzen.
- process_due_channel:1677 sendet Community direkt über community_timer; andere Kanäle verwenden maybe_send_promo_with_stats.
- prepare_channel_timers:632 lädt Einstellungen spätestens nach 30 Sekunden anhand Twitch-ID. Speichern und Live-Werte sind korrekt.
- promo_activity_ready_inner:2067 prüft min_messages, Aktivitätsfenster und new_chatters; neue Chatter werden beim ersten Versand aktuell nicht geprüft.
- mark_promo_sent:1993 setzt Nachrichtenzähler zurück und markiert aktive Chatter als gesehen.
- get_new_chatters_in_window_inner:2130 bezieht auch Session-Viewer ein, während update_seen_chatters_inner nur activity markiert. Semantik prüfen, wenn für das Einhalten der Grenzen notwendig.
- Weitere Community-Abweichungen bei pitch_channel_limit_ok, partner_channel_limit_ok und build_promo_text prüfen. Redaktionelle Kanaltexte und eigene Einstellungswerte sind keine Timer-Ausnahme und bleiben erhalten.
- maybe_send_viewer_spike_promo:2193 prüft bisher nur eine Roh-Nachricht. Keine alternative Werbeschleife darf die jetzt verlangten Grenzen umgehen.
- PromoTimers.tsx:54 erklärt bisher die ausdrücklich verworfene Ausnahme; wahrheitsgemäß korrigieren.

## Abnahme und Lieferung

Koordinationshinweis 18:55Z: Bereits laufender fremder Cargo-Build PID 1451320 in /var/lib/twitchbuild/tb-dach-lock-20260912/rust. Parallelthread 7c8402a1-e7c5-45e4-85cb-9d64f4146e57 verwaltet Twitch-Ankündigungen und baut 1fd4221c, dort ist der Community-Bypass noch vorhanden. Neue Ankündigungspfade in derselben promos.rs unbedingt berücksichtigen. Dieser gehört nicht zu unserem Paket. Keine Release-Builds starten; isolierte Debug-Checks nur ressourcenschonend. Hauptsession koordiniert Ownership, keine geteilten Ressourcen anfassen. Nachfolger-Release erst nach Peer-Abschluss.

Lokale Anweisungen und Skills lesen. Bestehende Tests nachziehen, gezielte vorhandene Tests, cargo check, fmt und clippy ausführen. Konkrete Befehle und Logs für unabhängige Abnahme bereitstellen. Kein paralleler Cargo-Release-Build, keine geteilten Build-Artefakte verändern. Änderungen müssen die eingestellten Mindestnachrichten und neuen Chatter auch beim ersten Timer-Versand sowie ViewerSpike respektieren und Community-Ausnahmen in relevanten Werbegrenzen entfernen. Keine harte Zusatzschwelle erfinden. Alle tatsächlich geprüften Regeln im REPORT.md beschreiben.

Nach Implementierung `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-promo-admin-grenzen --base 27d9482b --head HEAD` gegen eigene Arbeit durchführen und berechtigte Funde beheben. Dieser Gate-Pfad ist bestätigt; dokumentierte andere Pfade fehlen. Kein zusätzlicher Abnahme-Reviewthread: unabhängige Rust-/Intent-Abnahme koordiniert die Hauptsession separat. Noch NICHT mergen, deployen oder Dienste neu starten. Branch committen, Checks und Commit im REPORT.md dokumentieren. Register aktualisieren. Bei Blocker im selben Worktree bleiben und präzise melden.

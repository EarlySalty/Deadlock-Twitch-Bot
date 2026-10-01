# Nachtrag 3 (2026-09-18): Twitch-Einstellung bleibt Sache des Streamers, Chat-Hinweis vor Werbung

## Streichung (Paket A und B)

- `plan.suggestion` und die Hinweiszeile mit Einstellungsvorschlag und Link zum Twitch-Werbungs-Manager entfallen ersatzlos. Wie viel Werbung läuft, entscheidet allein der Streamer bei Twitch; wir bewerten und kommentieren das nicht.
- `plan.fit` bleibt als internes Signal, damit der Bot sein Vorgehen an den Plan anpasst (Pausen aufsparen oder vorziehen). Im Dashboard taucht es nur im Status-Satz auf, was der Bot gerade tut, nie als Empfehlung.

## Neu: Paket D, Chat-Hinweis vor der Werbung (startet nach Fertigmeldung von A, auf dessen Branch-Stand)

Ziel: der Chat weiß kurz vorher Bescheid, dass gleich Werbung kommt.

0. Der Hinweis läuft nur, wenn der Werbemanager eingeschaltet ist. Ist er aus, schreibt der Bot nie etwas zu Werbung in den Chat, auch nicht bei von Twitch geplanter Werbung.
1. Rund 30 Sekunden vor einer Werbung, die der Bot selbst startet oder vorzieht, schreibt der Bot eine kurze Zeile in den Chat. Bei von Twitch geplanter Werbung, die der Bot nicht bewegt, kommt der Hinweis im selben Abstand vor `next_ad_at`.
2. Eine Zeile je Werbung, nie eine zweite, keine Nachricht nach der Werbung. Wird die Werbung nach dem Hinweis doch noch verschoben (nur ein Raid darf das), folgt keine Korrektur-Nachricht; der nächste Hinweis kommt erst zur nächsten Werbung.
3. Feste Texte in mehreren rotierenden Varianten, kein LLM, dieselbe Variante nicht zweimal hintereinander. Ton locker, kurz, nicht bottig, nennt Dauer und wenn bekannt den Anlass. Startvorschläge, Endfassung über die Schreib-Skills:
   - "Kurze Werbung in 30 Sekunden, {dauer} Sekunden, dann geht's weiter."
   - "Queue läuft, perfekter Moment: gleich {dauer} Sekunden Werbung."
   - "Gleich kommt kurz Werbung. Holt euch was zu trinken, in {dauer} Sekunden sind wir wieder da."
   - "Werbung in 30 Sekunden, dafür bleibt das Match gleich frei."
   - "Kurze Pause für die Werbung, {dauer} Sekunden. Bis gleich."
4. Schalter "Chat vor Werbung informieren" in den Einstellungen des Werbemanagers, Default an. Migration als neue Spalte, Dashboard-Schalter in der Status-Karte oder den Feineinstellungen (Absprache mit dem Stand von Paket B, dessen Dateien D für diesen einen Schalter anfassen darf, sobald B fertig gemeldet hat).
5. Gesendet wird über den bestehenden Chat-Sendeweg des Bots, kein neuer Sender. Der Hinweis wird im Verlauf mitgeschrieben.
6. Abonnenten und Turbo-Nutzer sehen keine Werbung; der Text behauptet deshalb nicht, dass alle sie sehen.
